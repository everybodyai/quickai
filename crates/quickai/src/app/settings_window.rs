//! The ⚙ Settings window: the downloaded-models folder and list, live traffic, and the
//! "Advanced: llama.cpp" block (binary override, installed builds, build look-up, extra arguments).
//!
//! It is a window of its own, not a panel drawn over the main one: it can be put beside the main window,
//! made any size, and left open while the main window is used. (It is an immediate viewport: the frame
//! that draws the main window draws it too, so both always show the same state.)

use std::path::PathBuf;

use eframe::egui;

use quickai_core::form::{FormField, FormModel};

use super::{App, PendingDelete};
use crate::hardware;

/// What the window is called to the window manager: its title bar, and what a taskbar lists.
pub(super) const SETTINGS_TITLE: &str = "QuickAI Settings";

/// The size the window opens at, and the least it can be made. It is tall enough for the longest of what it
/// holds (an engine's Advanced section, open) without scrolling.
const OPENS_AT: [f32; 2] = [500.0, 700.0];
const LEAST: [f32; 2] = [400.0, 320.0];

/// Which window the Settings are: the id the display knows it by.
pub(super) fn settings_viewport() -> egui::ViewportId {
    egui::ViewportId::from_hash_of("quickai_settings")
}

impl App {
    /// Draws the Settings window, if it is open: a window of its own (see the head of this file).
    pub(super) fn show_settings_window(&mut self, ctx: &egui::Context) {
        if !self.show_settings {
            return;
        }
        let id = settings_viewport();
        // asked for again while open, somewhere behind the main window: they come to the front
        if std::mem::take(&mut self.raise_settings) {
            ctx.send_viewport_cmd_to(id, egui::ViewportCommand::Focus);
        }
        let form = self.form();
        let mut close = false;
        let builder = egui::ViewportBuilder::default()
            .with_title(SETTINGS_TITLE)
            .with_app_id(crate::APP_ID)
            .with_icon(crate::app_icon())
            .with_inner_size(OPENS_AT)
            .with_min_inner_size(LEAST);
        ctx.show_viewport_immediate(id, builder, |ui, class| {
            if class == egui::ViewportClass::EmbeddedWindow {
                // A context that cannot open a window (a test draws on one with no display) shows it inside
                // the main window, as a window that is as tall as what is in it, up to the window it is in,
                // with its Close button under it.
                let room = (ui.ctx().content_rect().height() - 120.0).max(200.0);
                egui::ScrollArea::vertical()
                    .id_salt("settings_scroll")
                    .max_height(room)
                    .auto_shrink([false, true])
                    .show(ui, |ui| self.show_settings_body(ui, &form));
                ui.add_space(10.0);
                if ui.button("Close").clicked() {
                    close = true;
                }
            } else {
                // the window manager's own close button asks to close it, and it is closed by not being drawn
                if ui.input(|input| input.viewport().close_requested()) {
                    close = true;
                }
                egui::Panel::bottom("settings_footer").show(ui, |ui| {
                    ui.add_space(6.0);
                    if ui.button("Close").clicked() {
                        close = true;
                    }
                    ui.add_space(6.0);
                });
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::ScrollArea::vertical()
                        .id_salt("settings_scroll")
                        .auto_shrink([false, false])
                        .show(ui, |ui| self.show_settings_body(ui, &form));
                });
            }
            // every delete button of the window asks its question in the window, where it was pressed
            self.show_confirm_delete_modal(ui.ctx());
        });
        if close {
            self.close_settings();
        }
    }

    /// Opens the Settings, or brings them to the front when they are open already (a window of their
    /// own can be behind the main one).
    pub(super) fn open_settings(&mut self) {
        self.raise_settings = self.show_settings;
        self.show_settings = true;
    }

    /// Closes the Settings and writes down what was changed in them, however they were closed: by their
    /// Close button, their window's own close button or the ⚙ icon. A question asked in them (a delete
    /// waiting for its yes) goes with them: nobody is left to answer it.
    pub(super) fn close_settings(&mut self) {
        self.show_settings = false;
        self.raise_settings = false;
        self.pending_delete = None;
        self.delete_of_companion = false;
        // what the manager of builds shows of the disk is not looked at while the window is closed: it is read again when it is opened
        self.runtime.disk.stale = true;
        self.as_companion(|app| app.runtime.disk.stale = true);
        self.save_settings();
    }

    /// What the window holds, in the order it is read: the engine, the models, the traffic proxy, the
    /// engine's Advanced section and what went wrong.
    fn show_settings_body(&mut self, ui: &mut egui::Ui, form: &Option<FormModel>) {
        ui.label(egui::RichText::new("App").strong());
        ui.add_space(4.0);
        self.show_engine_row(ui);

        if self.lists_models() {
            self.show_named_models_list(ui);
        } else {
            if let Some(field) = form.as_ref().and_then(|f| f.field("models_dir")) {
                let label = ui.label(format!("{}:", field.label));
                if let Some(help) = &field.help {
                    label.on_hover_text(help);
                }
            }
            ui.add(
                egui::TextEdit::singleline(&mut self.models_dir_input)
                    .desired_width(ui.available_width()),
            );
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
                        self.settings_error = Some(format!(
                            "Could not create/use {}",
                            self.models_dir_input.trim()
                        ));
                    }
                }
                if ui.button("Reset to default").clicked() {
                    let default_dir = self.default_models_dir();
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
                        "Models fetched via the \"Get\" box, \"Look up online\", or \
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
        }

        ui.add_space(10.0);
        let active = self.session.is_active();
        let server = self.server_name();
        // an engine with no conversation to show has nothing for the proxy to do
        if self.reads_traffic() {
            ui.add_enabled_ui(!active, |ui| {
                if ui
                    .checkbox(
                        &mut self.enable_traffic_proxy,
                        "Show live conversation traffic",
                    )
                    .on_hover_text(format!(
                        "Adds a small local proxy in front of {server} so a new \"📡 \
             Live\" button can show chat/completion requests and replies live, \
             from any client — the built-in chat UI or an external app via the \
             🔌 API. Off by default: clients then connect straight to \
             {server}, with no extra hop or overhead. Takes effect the next \
             time you click Start."
                    ))
                    .changed()
                {
                    self.save_settings();
                }
            });
            if active {
                ui.label(
                    egui::RichText::new(
                        "(Stop the server to change this — it applies on the next Start.)",
                    )
                    .small()
                    .color(super::theme::palette().muted),
                );
            }
        }

        // the program that goes with the engine, when one does (Open WebUI beside Ollama)
        self.show_companion_settings(ui);

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(6.0);

        let engine_name = self
            .registry
            .engine(&self.engine)
            .map(|e| e.doc.engine.name.clone())
            .unwrap_or_default();
        egui::CollapsingHeader::new(format!("Advanced: {engine_name}"))
            .default_open(false)
            .show(ui, |ui| {
                let advanced: Vec<FormField> = form
                    .as_ref()
                    .and_then(|f| f.section("advanced"))
                    .map(|s| s.fields.iter().filter(|f| f.visible).cloned().collect())
                    .unwrap_or_default();
                self.show_fields(ui, &advanced, Some(&engine_name));
            });

        if let Some(msg) = &self.settings_error {
            ui.add_space(6.0);
            ui.colored_label(super::theme::palette().bad, msg);
        }
    }
}

impl App {
    /// The row that says which engine runs the models and lets the person change it. It is locked
    /// while something that belongs to the engine in use is going on.
    fn show_engine_row(&mut self, ui: &mut egui::Ui) {
        let mut engines: Vec<(String, String)> = self
            .registry
            .selectable()
            .map(|e| (e.id.clone(), e.doc.engine.name.clone()))
            .collect();
        // llama.cpp comes first: it is the engine of a first run
        engines.sort_by_key(|(id, _)| id != crate::settings::DEFAULT_ENGINE);
        let locked = self.engine_locked();
        let current = self.engine_name();
        let mut chosen: Option<String> = None;
        ui.horizontal(|ui| {
            ui.label("Engine:").on_hover_text(
                "Which program runs the models: llama.cpp, which comes with QuickAI, or Ollama, \
                 which QuickAI downloads only when you ask. Each keeps its own settings.",
            );
            ui.add_enabled_ui(!locked, |ui| {
                egui::ComboBox::from_id_salt("engine_combo")
                    .selected_text(current)
                    .show_ui(ui, |ui| {
                        for (id, name) in &engines {
                            if ui.selectable_label(*id == self.engine, name).clicked() {
                                chosen = Some(id.clone());
                            }
                        }
                    });
            });
        });
        if locked {
            let why = if self.session.is_active() {
                "(Stop the server to change this.)"
            } else {
                "(Wait for what is being fetched to finish to change this.)"
            };
            ui.label(
                egui::RichText::new(why)
                    .small()
                    .color(super::theme::palette().muted),
            );
        }
        if let Some(id) = chosen {
            self.switch_engine(&id);
        }
        ui.add_space(6.0);
    }

    /// The models the engine lists, each with the button that removes it (after a question).
    fn show_named_models_list(&mut self, ui: &mut egui::Ui) {
        let name = self.engine_name();
        let server = self.server_name();
        ui.add_space(4.0);
        ui.label(egui::RichText::new("Models").strong())
            .on_hover_text(format!(
                "The models {name} has, as {server} listed them. Removing one asks first, and \
                 needs {server} running: it does the removing."
            ));
        let mut to_delete: Option<PendingDelete> = None;
        if self.models.is_empty() {
            ui.label(
                egui::RichText::new(format!("{name} has not listed any models yet."))
                    .small()
                    .color(super::theme::palette().muted),
            );
        }
        let running = self.endpoint().is_some();
        egui::ScrollArea::vertical()
            .id_salt("listed_models_scroll")
            .auto_shrink([false, true])
            .max_height(list_scroll_height(ui, DELETE_LIST_MAX_ROWS))
            .show(ui, |ui| {
                for m in &self.models {
                    ui.horizontal(|ui| {
                        let loaded = self.is_loaded_model(&m.filename);
                        let tip = if loaded {
                            format!("{} is loaded: stop {server} to remove it.", m.display)
                        } else if !running {
                            format!("Start {server} to remove a model: it does the removing.")
                        } else {
                            format!("Remove this model from {name}")
                        };
                        if ui
                            .add_enabled(running && !loaded, egui::Button::new("🗑").small())
                            .on_hover_text(tip)
                            .clicked()
                        {
                            to_delete = Some(PendingDelete::Listed {
                                name: m.filename.clone(),
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
}

/// How many rows the "Downloaded models"/"Installed builds" delete lists
/// (⚙ Settings) show before scrolling, so a long download history doesn't
/// push the rest of the window down.
pub(super) const DELETE_LIST_MAX_ROWS: usize = 4;

/// Height for a `ScrollArea` that shows exactly `rows` list rows — each
/// row is a `ui.horizontal` containing an interactive widget, so its
/// height is governed by `interact_size.y` plus the spacing between rows.
pub(super) fn list_scroll_height(ui: &egui::Ui, rows: usize) -> f32 {
    (ui.spacing().interact_size.y + ui.spacing().item_spacing.y) * rows as f32
}

#[cfg(test)]
mod tests {
    use std::cell::RefCell;
    use std::collections::{BTreeMap, HashSet};
    use std::path::Path;
    use std::rc::Rc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use super::super::runtime_manager::Scan;
    use super::*;
    use crate::config::{build_registry, ConfigDirs};
    use crate::events::ServerState;
    use crate::prims::install::InstallSpec;
    use crate::settings::Settings;
    use crate::testkit::{scratch, wait_until};

    const GB: u64 = 1024 * 1024 * 1024;

    /// What a window of its own was asked to be and what it painted.
    struct Window {
        builder: egui::ViewportBuilder,
        shapes: Vec<egui::epaint::ClippedShape>,
        /// The size the display gave it.
        size: egui::Vec2,
    }

    #[derive(Default)]
    struct Shown {
        /// The windows drawn in the frame that is going.
        windows: BTreeMap<egui::ViewportId, Window>,
        /// What the person did in a window, for the next pass of it.
        events: BTreeMap<egui::ViewportId, Vec<egui::Event>>,
        /// The windows whose close button was pressed since they were last drawn.
        closed: HashSet<egui::ViewportId>,
        /// The size to open a window at instead of the one it asks for.
        size: Option<egui::Vec2>,
    }

    /// A display that opens a window of its own for a viewport, as the toolkit does on a screen: the code
    /// of the window runs as a pass of its own, on a screen of the size the window asked for, with the
    /// events the person gave that window. It writes down what each window asked for and painted: what a
    /// person would see in it. (A context with no display shows the same code as a window of the main one.)
    struct Display(Rc<RefCell<Shown>>);

    impl Display {
        fn attach(ctx: &egui::Context) -> Display {
            let shown = Rc::new(RefCell::new(Shown::default()));
            ctx.set_embed_viewports(false);
            let inner = Rc::clone(&shown);
            egui::Context::set_immediate_viewport_renderer(move |ctx, viewport| {
                let egui::ImmediateViewport {
                    ids,
                    builder,
                    mut viewport_ui_cb,
                } = viewport;
                let (events, closed, size) = {
                    let mut shown = inner.borrow_mut();
                    (
                        shown.events.remove(&ids.this).unwrap_or_default(),
                        shown.closed.remove(&ids.this),
                        shown.size,
                    )
                };
                let size = size
                    .or(builder.inner_size)
                    .unwrap_or(egui::vec2(500.0, 500.0));
                let mut info = egui::ViewportInfo::default();
                if closed {
                    info.events.push(egui::ViewportEvent::Close);
                }
                let mut raw = egui::RawInput {
                    viewport_id: ids.this,
                    screen_rect: Some(egui::Rect::from_min_size(egui::Pos2::ZERO, size)),
                    events,
                    ..Default::default()
                };
                raw.viewports.insert(ids.this, info);
                let mut output = ctx.run_ui(raw, |ui| viewport_ui_cb(ui));
                output.textures_delta.clear();
                inner.borrow_mut().windows.insert(
                    ids.this,
                    Window {
                        builder,
                        shapes: output.shapes,
                        size,
                    },
                );
            });
            Display(shown)
        }

        /// What the person does in a window, in the next frame.
        fn give(&self, window: egui::ViewportId, events: Vec<egui::Event>) {
            self.0.borrow_mut().events.insert(window, events);
        }

        /// The person presses the window manager's close button on a window.
        fn press_close_button(&self, window: egui::ViewportId) {
            self.0.borrow_mut().closed.insert(window);
        }

        fn open_windows_at(&self, size: egui::Vec2) {
            self.0.borrow_mut().size = Some(size);
        }
    }

    fn app_in(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        App::build_in(
            settings,
            build_registry(&ConfigDirs::default()),
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    /// One frame of the application on the display: the main window, with the Settings window drawn as
    /// the application draws it. The windows the frame drew are the display's afterwards.
    fn frame(app: &mut App, ctx: &egui::Context, display: &Display) -> egui::FullOutput {
        display.0.borrow_mut().windows.clear();
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(760.0, 560.0),
            )),
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            egui::CentralPanel::default().show(ui, |ui| {
                app.show_header(ui);
                app.show_models_area(ui, false);
                app.show_controls(ui);
                app.show_run_bar(ui, ServerState::Idle, false);
                app.show_log_pane(ui);
            });
            app.show_settings_window(ui.ctx());
        });
        output.textures_delta.clear();
        output
    }

    /// A few frames with nothing done in them: a window that was just opened finds its size and where
    /// everything goes over its first frames, as a real one does.
    fn settle(app: &mut App, ctx: &egui::Context, display: &Display) {
        for _ in 0..4 {
            frame(app, ctx, display);
        }
    }

    /// Every text the shapes paint, with where.
    fn texts(shapes: &[egui::epaint::ClippedShape]) -> Vec<(String, egui::Rect)> {
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
        let mut into = Vec::new();
        for clipped in shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    fn said(shapes: &[egui::epaint::ClippedShape]) -> String {
        texts(shapes)
            .into_iter()
            .map(|(text, _)| text)
            .collect::<Vec<_>>()
            .join("\n")
    }

    /// Where `text` is painted in the window of the display, if it is.
    fn where_is(display: &Display, text: &str) -> Option<egui::Rect> {
        let shown = display.0.borrow();
        let window = shown.windows.get(&settings_viewport())?;
        texts(&window.shapes)
            .into_iter()
            .find(|(painted, _)| painted == text)
            .map(|(_, rect)| rect)
    }

    /// The person clicks `at` in the Settings window: the pointer goes there, then presses, then lets go
    /// (a frame each, as a pointer arrives).
    fn click_in_settings(app: &mut App, ctx: &egui::Context, display: &Display, at: egui::Pos2) {
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
            vec![],
        ] {
            display.give(settings_viewport(), events);
            frame(app, ctx, display);
        }
    }

    #[test]
    fn p3_f85_the_settings_are_a_window_of_their_own_and_none_of_them_is_drawn_in_the_main_window()
    {
        let dir = scratch("f85");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);

        // closed, the display is asked for nothing
        let out = frame(&mut app, &ctx, &display);
        assert!(display.0.borrow().windows.is_empty());
        assert!(!out.viewport_output.contains_key(&settings_viewport()));
        assert!(said(&out.shapes).contains("Start"), "the main window");

        // open, it is asked for one window, named and sized, with the icon of the application...
        app.toggle_settings();
        let out = frame(&mut app, &ctx, &display);
        assert!(out.viewport_output.contains_key(&settings_viewport()));
        {
            let shown = display.0.borrow();
            assert_eq!(shown.windows.len(), 1);
            let window = &shown.windows[&settings_viewport()];
            assert_eq!(window.builder.title.as_deref(), Some("QuickAI Settings"));
            assert_eq!(window.builder.app_id.as_deref(), Some(crate::APP_ID));
            assert!(window.builder.icon.is_some(), "the icon of the application");
            assert!(
                window.builder.decorations != Some(false)
                    && window.builder.resizable != Some(false),
                "the window manager's own title bar, and a size the person can change"
            );
            let size = window.builder.inner_size.expect("a size to open at");
            assert!(size.x >= 400.0 && size.y >= 500.0, "{size:?}");
            // ...and what is in it is there, and in no other window
            let inside = said(&window.shapes);
            for text in ["App", "Engine:", "Show live conversation traffic", "Close"] {
                assert!(inside.contains(text), "{text:?} in the window: {inside}");
            }
            let main = said(&out.shapes);
            for text in [
                "Show live conversation traffic",
                "Engine:",
                "Reset to default",
            ] {
                assert!(!main.contains(text), "{text:?} in the main window: {main}");
            }
            assert!(main.contains("Start") && main.contains("Port:"), "{main}");
        }

        // closed again, the window is not asked for and so goes
        app.toggle_settings();
        let out = frame(&mut app, &ctx, &display);
        assert!(display.0.borrow().windows.is_empty());
        assert!(!out.viewport_output.contains_key(&settings_viewport()));
    }

    #[test]
    fn p3_f86_however_the_settings_window_is_closed_what_was_changed_in_it_is_written() {
        let dir = scratch("f86");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let saved = |name: &str| -> serde_json::Value {
            std::fs::read_to_string(dir.join("settings.json"))
                .ok()
                .and_then(|text| serde_json::from_str(&text).ok())
                .map(|json: serde_json::Value| json["values"][name].clone())
                .unwrap_or(serde_json::Value::Null)
        };
        for way in ["its Close button", "the close button of the window manager"] {
            let mut app = app_in(&dir, "comfyui");
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            assert!(app.show_settings && !display.0.borrow().windows.is_empty());
            let python = format!("/usr/bin/python3.{}", way.len());
            app.values.set("python", python.clone());
            if way == "its Close button" {
                let close = where_is(&display, "Close")
                    .expect("a Close button")
                    .center();
                click_in_settings(&mut app, &ctx, &display, close);
            } else {
                display.press_close_button(settings_viewport());
                frame(&mut app, &ctx, &display);
            }
            assert!(!app.show_settings, "closed with {way}");
            assert_eq!(
                saved("comfyui.python"),
                serde_json::Value::String(python),
                "what was typed is kept, closed with {way}"
            );
            let out = frame(&mut app, &ctx, &display);
            assert!(
                display.0.borrow().windows.is_empty()
                    && !out.viewport_output.contains_key(&settings_viewport()),
                "the window is gone once closed with {way}"
            );
        }
    }

    #[test]
    fn p3_f87_a_delete_asked_in_the_settings_is_asked_in_their_window_and_goes_with_it() {
        let dir = scratch("f87");
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::File::create(models.join("zeta-model.gguf"))
            .unwrap()
            .set_len(1_000_000)
            .unwrap();
        let mut app = app_in(&dir, "llamacpp");
        assert!(app.models.iter().any(|m| m.filename == "zeta-model.gguf"));
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);

        // the trash can of the model is pressed in the window...
        let bin = where_is(&display, "🗑").expect("a trash can for a downloaded model");
        click_in_settings(&mut app, &ctx, &display, bin.center());
        assert!(app.pending_delete.is_some(), "the question is waiting");
        // ...and the question is in that window, not in the main one
        let out = frame(&mut app, &ctx, &display);
        let inside = said(&display.0.borrow().windows[&settings_viewport()].shapes);
        assert!(
            inside.contains("Delete model?") && inside.contains("zeta-model.gguf"),
            "{inside}"
        );
        assert!(!said(&out.shapes).contains("Delete model?"));
        assert!(
            models.join("zeta-model.gguf").exists(),
            "nothing is deleted before the yes"
        );

        // the window is closed with the question open: nobody is left to answer it, and it is not asked again
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        assert!(!app.show_settings && app.pending_delete.is_none());
        app.toggle_settings();
        frame(&mut app, &ctx, &display);
        let inside = said(&display.0.borrow().windows[&settings_viewport()].shapes);
        assert!(!inside.contains("Delete model?"), "{inside}");
        assert!(models.join("zeta-model.gguf").exists());
    }

    #[test]
    fn p3_f88_asked_for_again_while_open_the_settings_come_to_the_front_and_are_not_opened_twice() {
        let dir = scratch("f88");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let commands = |out: &egui::FullOutput| {
            out.viewport_output
                .get(&settings_viewport())
                .map(|v| v.commands.clone())
                .unwrap_or_default()
        };

        // opened, the window is not told to take the focus (it is given it by being new)
        app.invoke("open:settings");
        let out = frame(&mut app, &ctx, &display);
        assert!(app.show_settings && display.0.borrow().windows.len() == 1);
        assert!(!commands(&out).contains(&egui::ViewportCommand::Focus));

        // asked for again (a button of the main window says "open the settings"), it comes to the front, once
        app.invoke("open:settings");
        let out = frame(&mut app, &ctx, &display);
        assert!(
            commands(&out).contains(&egui::ViewportCommand::Focus),
            "{:?}",
            commands(&out)
        );
        assert_eq!(display.0.borrow().windows.len(), 1, "one window");
        let out = frame(&mut app, &ctx, &display);
        assert!(
            !commands(&out).contains(&egui::ViewportCommand::Focus),
            "once"
        );
        assert!(app.show_settings, "asking is not closing");
    }

    #[test]
    fn p3_f89_the_close_button_stays_at_the_foot_of_a_window_that_is_too_short_for_what_is_in_it() {
        let dir = scratch("f89");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        // a person made the window small: the settings scroll, and Close is where it was
        display.open_windows_at(egui::vec2(480.0, 150.0));
        settle(&mut app, &ctx, &display);
        let close = where_is(&display, "Close").expect("a Close button");
        let size = display.0.borrow().windows[&settings_viewport()].size;
        assert!(
            close.max.y <= size.y && close.min.y > size.y - 60.0,
            "Close is at the foot of a window {size:?} tall: {close:?}"
        );
        // and in a window of the size it opens at it is in the same place, at its foot
        display.open_windows_at(egui::vec2(500.0, 620.0));
        settle(&mut app, &ctx, &display);
        let close = where_is(&display, "Close").expect("a Close button");
        assert!(
            close.min.y > 620.0 - 60.0 && close.max.y <= 620.0,
            "{close:?}"
        );
        // the last thing the window says is above it, and not under it
        let advanced = where_is(&display, "Advanced: llama.cpp").expect("the Advanced section");
        assert!(advanced.max.y < close.min.y, "{advanced:?} {close:?}");
    }

    static SCANS: AtomicUsize = AtomicUsize::new(0);

    /// A reader of the disk that finds nothing and counts how many times it was asked.
    fn counting_reader(_: &InstallSpec) -> Scan {
        SCANS.fetch_add(1, Ordering::SeqCst);
        Scan {
            builds: Vec::new(),
            unfinished: Vec::new(),
        }
    }

    #[test]
    fn p3_f92_the_builds_are_read_from_the_disk_again_when_the_settings_window_is_opened_again() {
        // The window of the Settings is a window of its own, and the passes it counts begin again from none each time it is
        // opened: what tells the manager of builds that it was away (a build installed, an install that stopped, meanwhile)
        // must not be a count of the window's own. (Found by the GUI suite, TC-CFR-013: a stopped install was not listed
        // when the Settings were opened again.)
        let dir = scratch("f92");
        let mut app = app_in(&dir, "comfyui");
        app.runtime.disk.reader = Some(counting_reader);
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        let scans = || SCANS.load(Ordering::SeqCst);
        let scans_after = |app: &mut App, wanted: usize, what: &str| {
            wait_until(Duration::from_secs(10), what, || {
                frame(app, &ctx, &display);
                scans() == wanted
            });
        };

        // opened, with the Advanced section open, the disk is read once and not again while the window goes on
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let header = where_is(&display, "Advanced: ComfyUI").expect("the Advanced section");
        click_in_settings(&mut app, &ctx, &display, header.center());
        scans_after(&mut app, 1, "the first scan");
        settle(&mut app, &ctx, &display);
        assert_eq!(scans(), 1, "no scan without a reason");

        // closed by any of the ways, and opened again: it is read again
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        assert!(!app.show_settings);
        app.toggle_settings();
        scans_after(&mut app, 2, "the scan after the window was opened again");

        // and it is, with nothing but the frames of the main window in between, when the window went away some other way
        app.show_settings = false;
        for _ in 0..6 {
            frame(&mut app, &ctx, &display);
        }
        app.show_settings = true;
        scans_after(
            &mut app,
            3,
            "the scan after the window was away for a while",
        );
    }

    // ---- the program that goes with the engine (work package 4.16) ----------------------------------------

    /// What the Settings window painted last.
    fn in_settings(display: &Display) -> String {
        said(&display.0.borrow().windows[&settings_viewport()].shapes)
    }

    /// A frame as the application does it between draws: what its parts have to say, then the drawing.
    fn frame_polled(app: &mut App, ctx: &egui::Context, display: &Display) {
        app.poll_runtime_manager();
        app.poll_companion(ctx);
        frame(app, ctx, display);
    }

    /// The launcher of an installed build of Open WebUI, as the document names it for this system.
    fn launcher_file() -> &'static str {
        if cfg!(windows) {
            "open-webui.cmd"
        } else {
            "open-webui"
        }
    }

    fn app_with_companion(dir: &Path, enabled: bool) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let registry = build_registry(&ConfigDirs::default());
        let values: std::collections::BTreeMap<String, quickai_core::value::Value> = [(
            "openwebui.enabled".to_string(),
            quickai_core::value::Value::Bool(enabled),
        )]
        .into();
        settings
            .set_engine_values(registry.engine("openwebui").unwrap(), &values)
            .unwrap();
        App::build_in(
            settings,
            registry,
            16 * GB,
            Vec::new(),
            Some(dir.join("settings.json")),
            Some(dir.join("data").display().to_string()),
        )
    }

    #[test]
    fn p3_t36_the_settings_of_ollama_offer_open_webui_and_those_of_the_other_engines_do_not() {
        for (engine, offered) in [("llamacpp", false), ("ollama", true), ("comfyui", false)] {
            let dir = scratch(&format!("t36-{engine}"));
            let mut app = app_in(&dir, engine);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            let inside = in_settings(&display);
            assert_eq!(
                inside.contains("Chat UI: Open WebUI"),
                offered,
                "{engine}: {inside}"
            );
            // the engine row offers the engines, and the companion is not one of them
            assert!(!inside.contains("Open WebUI") || offered, "{engine}");
            assert_eq!(app.companion.is_some(), offered, "{engine}");
        }
        // and the section, opened, says what it is and what it asks
        let dir = scratch("t36-open");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let header = where_is(&display, "Chat UI: Open WebUI").expect("the section");
        click_in_settings(&mut app, &ctx, &display, header.center());
        settle(&mut app, &ctx, &display);
        let inside = in_settings(&display);
        for text in [
            "A chat page for your models",
            "License: Open WebUI License",
            "openwebui.org",
            "Start Open WebUI with Ollama",
            "Port:",
            "Open WebUI's internet access:",
            "Look up Open WebUI builds",
            "Installs a Python environment with",
            "Open WebUI and what it needs",
        ] {
            let text = text.replace("openwebui.org", "openwebui.com");
            assert!(inside.contains(&text), "{text:?} in: {inside}");
        }
        assert!(
            inside.contains("Open WebUI launcher:") && !inside.contains("Installed builds"),
            "its manager is the manager of an engine: the box for the program, and no builds yet: {inside}"
        );
    }

    #[test]
    fn p3_t37_the_section_is_open_when_it_is_wanted_and_not_there_and_closed_when_it_is_not() {
        let open = |dir: &Path, enabled: bool, installed: bool| -> bool {
            if installed {
                let build = dir.join("data/openwebui/0.11.4");
                std::fs::create_dir_all(&build).unwrap();
                std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
            }
            let mut app = app_with_companion(dir, enabled);
            assert_eq!(app.companion_installed(), installed);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.toggle_settings();
            settle(&mut app, &ctx, &display);
            in_settings(&display).contains("Start Open WebUI with Ollama")
        };
        assert!(
            open(&scratch("t37a"), true, false),
            "asked for and not there: the thing to do"
        );
        assert!(!open(&scratch("t37b"), false, false), "not asked for");
        assert!(
            !open(&scratch("t37c"), true, true),
            "asked for and there: nothing to do"
        );
        // the button that sets it up opens the Settings with the section open, whatever it was (and the person can fold it)
        for (what, enabled, installed) in
            [("a", false, false), ("b", false, true), ("c", true, true)]
        {
            let dir = scratch(&format!("t37{what}2"));
            if installed {
                let build = dir.join("data/openwebui/0.11.4");
                std::fs::create_dir_all(&build).unwrap();
                std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
            }
            let mut app = app_with_companion(&dir, enabled);
            let ctx = egui::Context::default();
            let display = Display::attach(&ctx);
            app.set_up_companion();
            settle(&mut app, &ctx, &display);
            assert!(
                in_settings(&display).contains("Start Open WebUI with Ollama"),
                "the button's click, with the box {enabled} and the program {installed}"
            );
            assert!(
                !app.companion_section_wanted,
                "the wish is for one drawing, not for ever"
            );
        }
    }

    #[test]
    fn p3_t38_ticking_the_box_changes_the_companions_values_only_and_they_are_written_with_the_others(
    ) {
        let dir = scratch("t38");
        let mut app = app_with_companion(&dir, false);
        let ollama_before = app.values.map().clone();
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        settle(&mut app, &ctx, &display);
        let header = where_is(&display, "Chat UI: Open WebUI").expect("the section");
        click_in_settings(&mut app, &ctx, &display, header.center());
        settle(&mut app, &ctx, &display);
        assert!(!app.companion_enabled());

        let label = where_is(&display, "Start Open WebUI with Ollama").expect("the box");
        click_in_settings(&mut app, &ctx, &display, label.center());
        settle(&mut app, &ctx, &display);
        assert!(app.companion_enabled(), "the box is ticked");
        assert_eq!(
            app.values.map(),
            &ollama_before,
            "Ollama's own values are what they were: the click was the companion's"
        );
        assert!(app.engine == "ollama" && !app.in_companion);

        // closing the window writes both
        display.press_close_button(settings_viewport());
        frame(&mut app, &ctx, &display);
        let saved: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(saved["values"]["openwebui.enabled"], true, "{saved}");
        assert_eq!(saved["values"]["app.engine"], "ollama", "{saved}");
    }

    #[test]
    fn p3_t39_a_build_of_the_companion_is_listed_and_deleted_in_its_own_section_and_ollamas_are_untouched(
    ) {
        let dir = scratch("t39");
        // a build of each: Ollama's, and Open WebUI's
        let ollama_build = dir.join("data/ollama/v0.35.1-ollama-linux-amd64");
        std::fs::create_dir_all(ollama_build.join("bin")).unwrap();
        std::fs::write(ollama_build.join("bin/ollama"), "#!/bin/sh\n").unwrap();
        let build = dir.join("data/openwebui/0.11.4");
        std::fs::create_dir_all(build.join("venv")).unwrap();
        std::fs::write(build.join(launcher_file()), "#!/bin/sh\n").unwrap();
        std::fs::write(build.join("venv/pyvenv.cfg"), "home = x\n").unwrap();
        let mut app = app_with_companion(&dir, true);
        assert!(
            app.companion_installed(),
            "found where the document says builds are kept"
        );
        let ollama_binary = app.binary.clone();
        let ctx = egui::Context::default();
        let display = Display::attach(&ctx);
        app.toggle_settings();
        // wanted and there: the section is closed; open it
        settle(&mut app, &ctx, &display);
        let header = where_is(&display, "Chat UI: Open WebUI").expect("the section");
        click_in_settings(&mut app, &ctx, &display, header.center());
        // the disk is read by a thread of its own, and the frame takes the answer in
        wait_until(Duration::from_secs(10), "the builds to be listed", || {
            frame_polled(&mut app, &ctx, &display);
            in_settings(&display).contains("0.11.4")
        });
        let inside = in_settings(&display);
        assert!(inside.contains("Installed builds"), "{inside}");

        // the trash can of its build is pressed: the question is about Open WebUI's build
        let bin = where_is(&display, "🗑").expect("a trash can for the build");
        click_in_settings(&mut app, &ctx, &display, bin.center());
        assert!(app.pending_delete.is_some() && app.delete_of_companion);
        frame(&mut app, &ctx, &display);
        let inside = in_settings(&display);
        assert!(
            inside.contains("Delete Open WebUI build?") && inside.contains("0.11.4"),
            "{inside}"
        );
        // and the yes deletes it, on a thread, and not Ollama's
        let yes = where_is(&display, "Delete").expect("the yes");
        click_in_settings(&mut app, &ctx, &display, yes.center());
        wait_until(Duration::from_secs(10), "the build to be deleted", || {
            frame_polled(&mut app, &ctx, &display);
            !build.exists()
        });
        wait_until(Duration::from_secs(10), "the delete to be taken in", || {
            frame_polled(&mut app, &ctx, &display);
            app.companion_slot()
                .is_some_and(|s| s.runtime.delete.is_none())
        });
        assert!(app.pending_delete.is_none() && !app.delete_of_companion);
        assert!(
            ollama_build.join("bin/ollama").is_file(),
            "Ollama's build is where it was"
        );
        assert_eq!(
            app.binary, ollama_binary,
            "and Ollama's program is the one it was"
        );
        assert!(app.runtime.delete.is_none() && app.values.text("bin_override").is_none());
        assert!(
            app.log_lines
                .iter()
                .any(|l| l.contains("Deleted Open WebUI build: 0.11.4")),
            "{:#?}",
            app.log_lines
        );
        assert!(!app.companion_installed(), "it is not installed now");
    }
}
