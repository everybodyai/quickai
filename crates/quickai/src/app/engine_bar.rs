//! Choosing the engine and getting it, from the main window: the engines as one segmented control in the toolbar
//! (a dropdown where the toolbar has no room for it, and the same choice in the Server menu), and, for an engine that
//! is downloaded on request and is not here yet, a section at the top of the page with one button that installs the
//! build its document offers first for this machine. Settings keeps the engine's row and its Advanced section, where
//! another version or a program of the person's own is chosen.

use std::path::Path;

use eframe::egui;
use quickai_core::acquire;

use super::runtime_manager::display_root;
use super::theme::{self, Role};
use super::App;
use crate::prims::feed::LookupSpec;
use crate::prims::install;

/// The room kept free between the buttons of the toolbar and the engine's control.
const TOOLBAR_GAP: f32 = 12.0;

impl App {
    /// The engines a person can choose, `(id, name, summary)`, llama.cpp first: it is the engine of a first run.
    pub(super) fn engine_choices(&self) -> Vec<(String, String, String)> {
        let mut engines: Vec<(String, String, String)> = self
            .registry
            .selectable()
            .map(|e| {
                (
                    e.id.clone(),
                    e.doc.engine.name.clone(),
                    e.doc.engine.summary.clone().unwrap_or_default(),
                )
            })
            .collect();
        engines.sort_by_key(|(id, _, _)| id != crate::settings::DEFAULT_ENGINE);
        engines
    }

    /// Why the engine cannot be changed now, when it cannot.
    fn engine_lock_reason(&self) -> Option<&'static str> {
        if !self.engine_locked() {
            None
        } else if self.session.is_active() {
            Some("Stop the server to change the engine.")
        } else {
            Some("Wait for what is being fetched to finish to change the engine.")
        }
    }

    /// The engine's control at the right of the toolbar, drawn in a layout that runs from right to left (beside
    /// Settings): the engines side by side when there is room, a dropdown when there is less, nothing when there is
    /// none (the Server menu has them too).
    pub(super) fn show_engine_switch(&mut self, ui: &mut egui::Ui, height: f32) {
        let engines = self.engine_choices();
        if engines.len() < 2 {
            return;
        }
        let names: Vec<&str> = engines.iter().map(|(_, name, _)| name.as_str()).collect();
        let chosen = engines
            .iter()
            .position(|(id, _, _)| *id == self.engine)
            .unwrap_or(0);
        let lock = self.engine_lock_reason();
        let room = ui.available_width() - TOOLBAR_GAP;
        let wide = theme::segmented_width(ui, &names);
        let label_width = 52.0;
        let mut picked: Option<usize> = None;
        if room >= wide + label_width {
            let hovers: Vec<String> = engines
                .iter()
                .map(|(_, name, summary)| match lock {
                    Some(why) => why.to_string(),
                    None if summary.is_empty() => format!("Use {name}."),
                    None => format!("Use {name}: {summary}"),
                })
                .collect();
            picked = theme::segmented(ui, &names, &hovers, chosen, lock.is_none(), height);
            ui.label(theme::muted("Engine")).on_hover_text(
                "Which program runs the models. Each engine keeps its own settings and models; one that is \
                 not on this computer yet is downloaded only when you ask.",
            );
        } else if room >= 120.0 {
            ui.add_enabled_ui(lock.is_none(), |ui| {
                egui::ComboBox::from_id_salt("toolbar_engine")
                    .selected_text(names[chosen])
                    .width(110.0)
                    .show_ui(ui, |ui| {
                        for (i, name) in names.iter().enumerate() {
                            if ui.selectable_label(i == chosen, *name).clicked() && i != chosen {
                                picked = Some(i);
                            }
                        }
                    })
                    .response
                    .on_hover_text(lock.unwrap_or("Which program runs the models."))
                    .on_disabled_hover_text(lock.unwrap_or_default());
            });
        }
        if let Some(i) = picked {
            self.switch_engine(&engines[i].0);
        }
    }

    /// The engines in the Server menu, the one in use ticked.
    pub(super) fn show_engine_menu(&mut self, ui: &mut egui::Ui) {
        let engines = self.engine_choices();
        if engines.len() < 2 {
            return;
        }
        let lock = self.engine_lock_reason();
        let mut picked: Option<String> = None;
        ui.label(theme::muted("Engine"));
        for (id, name, _) in &engines {
            let on = *id == self.engine;
            let item = ui.add_enabled(lock.is_none(), egui::RadioButton::new(on, name.as_str()));
            let item = match lock {
                Some(why) => item.on_disabled_hover_text(why),
                None => item,
            };
            if item.clicked() && !on {
                picked = Some(id.clone());
            }
        }
        if let Some(id) = picked {
            self.switch_engine(&id);
            ui.close();
        }
    }

    // ---- getting the engine ------------------------------------------------------------------------------

    /// What the page's Install button does: looks the builds up, and installs the first it offers for this machine
    /// when the answer comes (see `install_first_build`).
    pub(super) fn install_engine(&mut self, ctx: &egui::Context) {
        let (Some(spec), Some(texts)) = (
            self.engine_doc()
                .and_then(|e| LookupSpec::from_engine(e, &self.host_facts)),
            self.runtime_texts(),
        ) else {
            self.runtime.setup_failure =
                Some(format!("{} has no builds to download.", self.engine_name()));
            return;
        };
        self.runtime.setup_failure = None;
        self.start_build_lookup(spec, &texts, ctx);
        self.runtime.auto_install = Some(ctx.clone());
    }

    /// The look-up the page asked for has ended: the first build it offers that is whole (not changed since it was
    /// tested) and meant for this machine's graphics is installed, as its "Use" would have it. The feed offers the
    /// release the document was tested with first, so that is the one when the feed has it.
    pub(super) fn install_first_build(&mut self, failed: bool, ctx: &egui::Context) {
        if failed {
            self.runtime.setup_failure = Some(self.runtime.lookup_status.clone());
            return;
        }
        let rows = &self.runtime.lookup_results;
        let pick = rows
            .iter()
            .position(|r| !r.build.conflict && r.warning.is_none())
            .or_else(|| rows.iter().position(|r| !r.build.conflict));
        let (Some(i), Some(spec)) = (pick, self.install_spec()) else {
            self.runtime.setup_failure = Some(format!(
                "No {} build for this computer was found. Settings has the list, and a box for a program you \
                 already have.",
                self.engine_name()
            ));
            return;
        };
        let request = self.runtime.lookup_results[i].build.install_request(&[]);
        self.runtime.cancel = Some(spec.cancel.clone());
        self.runtime.install = Some(install::spawn_install_build(
            spec,
            request,
            super::calm_wake(ctx.clone()),
        ));
    }

    /// The section that gets an engine that is not here: what will be done, the button that does it, and then what
    /// it is doing or why it did not.
    pub(super) fn show_engine_setup(&mut self, ui: &mut egui::Ui) {
        if !self.engine_missing() {
            return;
        }
        let Some(engine) = self.engine_doc() else {
            return;
        };
        let name = engine.doc.engine.name.clone();
        let repo = acquire::feed_repo_label(engine).unwrap_or_default();
        let from_source = acquire::builds_from_source(engine);
        let root = acquire::root_dir(engine, &self.host_facts)
            .map(|r| display_root(Path::new(&r), &self.host_facts.home))
            .unwrap_or_default();
        let ticked = self.chosen_options();
        let ticked: Vec<&str> = ticked.iter().map(String::as_str).collect();
        let summary = acquire::install_summary_with(engine, &self.host_facts, &ticked);
        let looking = self.runtime.lookup_rx.is_some() && self.runtime.auto_install.is_some();
        let installing = self.runtime.install.is_some();
        // while the results of a look-up have the page the section is one row, and the results keep their room
        let compact = self.lookup.open;

        ui.add_space(10.0);
        theme::group(ui, &format!("Set up {name}"), |ui| {
            if !compact {
                let what = if from_source {
                    format!("its source from {repo}")
                } else {
                    format!("it from {repo}")
                };
                ui.label(format!(
                    "{name} is not installed yet. QuickAI downloads {what}, checks it, and keeps it in {root}: \
                     nothing else on the computer is changed, and nothing is downloaded until you click Install."
                ));
                if let Some(summary) = summary {
                    ui.label(theme::muted(summary));
                }
                ui.add_space(4.0);
            }
            ui.horizontal(|ui| {
                if compact {
                    ui.label(format!("{name} is not installed yet."));
                }
                let idle = !looking && !installing && self.runtime.delete.is_none();
                if theme::button_sized(
                    ui,
                    if idle { Role::Primary } else { Role::Normal },
                    idle,
                    format!("Install {name}"),
                    egui::vec2(0.0, 28.0),
                )
                .on_hover_text(format!(
                    "Download the {name} build QuickAI was tested with (or the newest, when that one is gone) \
                     for this computer, check it and install it."
                ))
                .clicked()
                {
                    self.install_engine(ui.ctx());
                }
                if theme::button_sized(
                    ui,
                    Role::Quiet,
                    true,
                    "Choose a version...",
                    egui::vec2(0.0, 28.0),
                )
                .on_hover_text(format!(
                    "Settings, Advanced: {name}: every build on offer, the ones installed, and a box for a \
                     {name} you already have."
                ))
                .clicked()
                                {
                    self.advanced_section_wanted = true;
                    self.open_settings();
                }
                if self.can_run_remote()
                    && theme::button_sized(
                        ui,
                        Role::Quiet,
                        true,
                        "Run on another computer...",
                        egui::vec2(0.0, 28.0),
                    )
                    .on_hover_text(format!(
                        "Use the {name} of a more powerful computer instead, reached over SSH: nothing is installed here."
                    ))
                    .clicked()
                {
                    self.remote.show_window = true;
                }
            });
            if looking {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(theme::muted(self.runtime.lookup_status.clone()));
                });
            }
            if let Some(job) = &self.runtime.install {
                ui.add_space(2.0);
                ui.horizontal(|ui| {
                    ui.colored_label(theme::palette().warn, job.progress_line());
                    if let Some(cancel) = &self.runtime.cancel {
                        if ui
                            .small_button("Cancel")
                            .on_hover_text(
                                "Stop the install. What was fetched so far is thrown away.",
                            )
                            .clicked()
                        {
                            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                        }
                    }
                });
                super::widgets::progress_bar(ui, job.fraction());
                if let Some(detail) = job.detail() {
                    ui.label(theme::muted(detail));
                }
            } else if let Some(why) = &self.runtime.setup_failure {
                ui.colored_label(theme::palette().bad, why);
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, Instant};

    use eframe::egui;
    use serde_json::json;

    use super::super::App;
    use crate::config::{build_registry, ConfigDirs};
    use crate::events::ServerState;
    use crate::prims::feed::{Build, BuildLookupEvent};
    use crate::settings::Settings;
    use crate::testkit::scratch;

    const GB: u64 = 1024 * 1024 * 1024;

    /// An application on `engine`, in `dir`, with the person's documents (if any) under `dir/cfg`: nothing of the real
    /// home is in sight, and no program is found on this machine's PATH by the name the document looks for.
    fn app_in(dir: &Path, engine: &str) -> App {
        let mut settings = Settings::default();
        settings.set_chosen_engine(engine);
        settings.models_dir_override = Some(dir.join("models").display().to_string());
        let registry = build_registry(&ConfigDirs {
            user: Some(dir.join("cfg")),
            ..ConfigDirs::default()
        });
        assert!(
            registry.diagnostics.iter().all(|d| !d.is_error()),
            "{:#?}",
            registry.diagnostics
        );
        let mut app = App::build_in(
            settings,
            registry,
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
        app.load_engine_state();
        // an Ollama the person runs themselves would be found on PATH: here there is none
        if engine != crate::settings::DEFAULT_ENGINE && !app.binary.starts_with(dir) {
            app.binary = PathBuf::from("/nowhere/ollama");
        }
        app
    }

    /// Every text a frame of the whole window painted, with where it is.
    fn painted(
        app: &mut App,
        ctx: &egui::Context,
        width: f32,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(width, 680.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| app.show_window(ui));
        output.textures_delta.clear();
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
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    /// The window after a few frames, which is when a window has measured itself.
    fn settled(app: &mut App, ctx: &egui::Context, width: f32) -> Vec<(String, egui::Rect)> {
        for _ in 0..3 {
            painted(app, ctx, width, Vec::new());
        }
        painted(app, ctx, width, Vec::new())
    }

    /// Where `text` is in the band of the toolbar.
    fn in_toolbar(found: &[(String, egui::Rect)], text: &str) -> Option<egui::Rect> {
        found
            .iter()
            .find(|(t, r)| t == text && r.center().y > 24.0 && r.center().y < 70.0)
            .map(|(_, r)| *r)
    }

    fn click(app: &mut App, ctx: &egui::Context, width: f32, at: egui::Pos2) {
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
            painted(app, ctx, width, events);
        }
    }

    #[test]
    fn the_engines_are_side_by_side_in_the_toolbar_and_a_click_changes_the_engine() {
        let dir = scratch("eb-switch");
        let mut app = app_in(&dir, "llamacpp");
        let ctx = egui::Context::default();
        let found = settled(&mut app, &ctx, 980.0);
        let at = |text: &str| {
            in_toolbar(&found, text)
                .unwrap_or_else(|| panic!("{text} is not in the toolbar: {found:?}"))
        };
        let (connect, settings) = (at("Connect an app"), at("Settings"));
        let engines: Vec<egui::Rect> = ["llama.cpp", "Ollama", "ComfyUI"]
            .iter()
            .map(|t| at(t))
            .collect();
        assert!(
            connect.right() < engines[0].left(),
            "{connect:?} {engines:?}"
        );
        assert!(
            engines.windows(2).all(|p| p[0].right() < p[1].left()),
            "{engines:?}"
        );
        assert!(
            engines[2].right() < settings.left(),
            "{engines:?} {settings:?}"
        );

        click(&mut app, &ctx, 980.0, engines[1].center());
        assert_eq!(app.engine, "ollama");
        let file: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("settings.json")).unwrap())
                .unwrap();
        assert_eq!(
            file["values"]["app.engine"], "ollama",
            "the choice is saved at once"
        );

        // while something of the engine in use is going on the choice stays as it is
        app.named.queued = Some(super::super::engine::Queued::Pull("qwen3:4b".into()));
        let found = settled(&mut app, &ctx, 980.0);
        let comfy = in_toolbar(&found, "ComfyUI").expect("the engines are still shown");
        click(&mut app, &ctx, 980.0, comfy.center());
        assert_eq!(app.engine, "ollama");
        app.named.queued = None;
        click(&mut app, &ctx, 980.0, comfy.center());
        assert_eq!(app.engine, "comfyui");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn at_every_width_the_engines_control_keeps_clear_of_the_buttons_and_of_settings() {
        for engine in ["llamacpp", "ollama", "comfyui"] {
            let dir = scratch(&format!("eb-widths-{engine}"));
            let mut app = app_in(&dir, engine);
            let ctx = egui::Context::default();
            for width in [640.0, 680.0, 760.0, 980.0, 1400.0] {
                let found = settled(&mut app, &ctx, width);
                let mut row: Vec<(String, egui::Rect)> = found
                    .iter()
                    .filter(|(_, r)| r.center().y > 24.0 && r.center().y < 70.0)
                    .cloned()
                    .collect();
                row.sort_by(|a, b| a.1.left().total_cmp(&b.1.left()));
                assert!(
                    row.windows(2).all(|p| p[0].1.right() <= p[1].1.left()),
                    "{engine} at {width}: texts of the toolbar over one another: {row:?}"
                );
                let settings = in_toolbar(&found, "Settings").expect("Settings");
                assert!(
                    settings.right() <= width,
                    "{engine} at {width}: {settings:?}"
                );
            }
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// A release of Ollama for Linux on x86-64 as the feed lists it, its file served by `files`.
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn release_of(
        files: &crate::testkit::files::FileServer,
        tag: &str,
        archive: &[u8],
    ) -> serde_json::Value {
        let path = format!("/dl/{tag}/ollama-linux-amd64.tar.zst");
        files.add(&path, archive.to_vec());
        json!({
            "tag_name": tag,
            "prerelease": false,
            "draft": false,
            "assets": [{
                "name": "ollama-linux-amd64.tar.zst",
                "size": archive.len(),
                "browser_download_url": files.url(&path),
                "digest": format!("sha256:{}", crate::testkit::archives::sha256_hex(archive)),
            }],
        })
    }

    /// The person's patch of the Ollama document that points its feed at `api`.
    fn feed_at(dir: &Path, api: &str) {
        std::fs::create_dir_all(dir.join("cfg/engines")).unwrap();
        std::fs::write(
            dir.join("cfg/engines/ollama.toml"),
            format!(
                "schema = 1\nkind = \"engine\"\nid = \"ollama\"\n\n[acquire]\nfeed = {{ use = \"github_release\", \
                 repo = \"ollama/ollama\", per_page = 10, show = 5, prereleases = false, digest = true, api = \"{api}\" }}\n"
            ),
        )
        .unwrap();
    }

    fn until(what: &str, mut done: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(60);
        while !done() {
            assert!(Instant::now() < deadline, "timed out waiting for {what}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }

    #[test]
    #[cfg(all(target_os = "linux", target_arch = "x86_64"))]
    fn the_install_button_of_the_page_gets_the_engine_and_the_section_goes_away() {
        use crate::testkit::archives::{ollama_linux_tree, tar_zst};
        use crate::testkit::fake_github::FakeGithub;
        use crate::testkit::files::FileServer;

        let dir = scratch("eb-install");
        let files = FileServer::start();
        let archive = tar_zst(&ollama_linux_tree());
        let github = FakeGithub::with_releases(json!([release_of(&files, "v0.99.0", &archive)]));
        feed_at(&dir, &github.base());
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        assert!(app.engine_missing());
        let found = settled(&mut app, &ctx, 980.0);
        let button = found
            .iter()
            .find(|(t, _)| t == "Install Ollama")
            .map(|(_, r)| *r)
            .unwrap_or_else(|| panic!("no Install button: {found:?}"));
        assert!(found.iter().any(|(t, _)| t == "Set up Ollama"));

        // one click: the builds are looked up and the first is installed, with nothing else to choose
        click(&mut app, &ctx, 980.0, button.center());
        assert!(
            app.runtime.lookup_rx.is_some(),
            "the click did not look the builds up"
        );
        until("the install to end", || {
            app.poll_runtime_manager();
            app.runtime.lookup_rx.is_none()
                && app.runtime.auto_install.is_none()
                && app.runtime.install.is_none()
        });
        assert_eq!(app.runtime.setup_failure, None);
        assert!(!app.engine_missing(), "{:?}", app.binary);
        assert!(
            app.binary.starts_with(dir.join("data/ollama")) && app.binary.ends_with("bin/ollama"),
            "{:?}",
            app.binary
        );
        assert_eq!(app.bin_override().as_deref(), Some(app.binary.as_path()));
        let found = settled(&mut app, &ctx, 980.0);
        assert!(
            !found.iter().any(|(t, _)| t == "Set up Ollama"),
            "the section is still there once the engine is"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn a_look_up_that_fails_says_why_in_the_section_and_installs_nothing() {
        use crate::testkit::fake_github::{FakeGithub, Mode};
        let dir = scratch("eb-fail");
        let github = FakeGithub::with_releases(json!([]));
        github.mode(Mode::Status(500));
        feed_at(&dir, &github.base());
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        app.install_engine(&ctx);
        until("the look-up to end", || {
            app.poll_runtime_manager();
            app.runtime.lookup_rx.is_none()
        });
        assert!(app.runtime.install.is_none());
        let why = app.runtime.setup_failure.clone().expect("a reason");
        assert!(why.starts_with("Lookup failed:"), "{why}");
        let found = settled(&mut app, &ctx, 980.0);
        assert!(
            found.iter().any(|(t, _)| *t == why),
            "the reason is not on the page: {found:?}"
        );

        // a feed with nothing for this machine says so, with where else to look
        let empty = FakeGithub::with_releases(json!([]));
        feed_at(&dir, &empty.base());
        let mut app = app_in(&dir, "ollama");
        app.install_engine(&ctx);
        until("the look-up to end", || {
            app.poll_runtime_manager();
            app.runtime.lookup_rx.is_none()
        });
        assert!(app.runtime.install.is_none());
        assert!(
            app.runtime
                .setup_failure
                .as_deref()
                .is_some_and(|w| w.starts_with("No Ollama build")),
            "{:?}",
            app.runtime.setup_failure
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_build_installed_is_the_first_that_is_whole_and_meant_for_this_machine() {
        let dir = scratch("eb-pick");
        let mut app = app_in(&dir, "ollama");
        let ctx = egui::Context::default();
        // served slowly, so that the install is still going when it is looked at
        let files = crate::testkit::files::FileServer::start();
        files.add_throttled("/slow.tar.zst", vec![0u8; 64 * 1024], 16);
        let build = |tag: &str, conflict: bool, url: String| Build {
            tag: tag.into(),
            asset_name: "ollama-linux-amd64.tar.zst".into(),
            url,
            size_bytes: Some(1),
            label: format!("{tag}-ollama-linux-amd64"),
            sha256: None,
            pin: None,
            tested: false,
            conflict,
            overlays: Vec::new(),
        };
        let looked_up = |app: &mut App, builds: Vec<Build>| {
            let (tx, rx) = crossbeam_channel::unbounded();
            app.runtime.lookup_rx = Some(rx);
            app.runtime.auto_install = Some(ctx.clone());
            for b in builds {
                tx.send(BuildLookupEvent::Found(b)).unwrap();
            }
            tx.send(BuildLookupEvent::Done).unwrap();
            app.poll_runtime_manager();
        };
        looked_up(
            &mut app,
            vec![
                build("v0.36.0", true, files.url("/slow.tar.zst")),
                build("v0.35.9", false, files.url("/slow.tar.zst")),
                build("v0.35.8", false, files.url("/slow.tar.zst")),
            ],
        );
        let job = app
            .runtime
            .install
            .as_ref()
            .expect("an install was started");
        assert_eq!(
            job.label, "v0.35.9-ollama-linux-amd64",
            "a file that changed after it was tested is never installed"
        );
        if let Some(cancel) = &app.runtime.cancel {
            cancel.store(true, std::sync::atomic::Ordering::SeqCst);
        }
        until("the install to stop", || {
            app.poll_runtime_manager();
            app.runtime.install.is_none()
        });
        assert_eq!(
            app.runtime.setup_failure, None,
            "a cancelled install is not a failure"
        );

        // one that fails says why on the page
        looked_up(
            &mut app,
            vec![build(
                "v0.35.9",
                false,
                "http://127.0.0.1:9/ollama-linux-amd64.tar.zst".into(),
            )],
        );
        until("the install to fail", || {
            app.poll_runtime_manager();
            app.runtime.install.is_none()
        });
        assert!(
            app.runtime.setup_failure.is_some(),
            "a failed install says why on the page"
        );
        assert!(app.engine_missing());
        assert_eq!(app.session.state(), ServerState::Idle);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ---- deleting a model, and another model for a server that runs -----------------------------------------

    /// Two model files of the person's in the models folder, and one that came with the application.
    fn with_models(app: &mut App, dir: &Path) {
        let folder = dir.join("models");
        std::fs::create_dir_all(&folder).unwrap();
        for name in ["alpha-Q4_K_M.gguf", "beta-Q4_K_M.gguf"] {
            std::fs::File::create(folder.join(name))
                .unwrap()
                .set_len(1024 * 1024)
                .unwrap();
        }
        let bundled = dir.join("bundled");
        std::fs::create_dir_all(&bundled).unwrap();
        std::fs::File::create(bundled.join("shipped-Q4_K_M.gguf"))
            .unwrap()
            .set_len(1024 * 1024)
            .unwrap();
        app.bundled_models_dir = bundled;
        app.models_dir = folder;
        app.models = app.scan_models();
        app.selected = 0;
    }

    fn index_of(app: &App, file: &str) -> usize {
        app.models
            .iter()
            .position(|m| m.filename == file)
            .unwrap_or_else(|| {
                panic!(
                    "{file} is not listed: {:?}",
                    app.models.iter().map(|m| &m.filename).collect::<Vec<_>>()
                )
            })
    }

    /// The window and the question of a delete asked in it, in one frame, as the application draws them.
    fn painted_with_modal(
        app: &mut App,
        ctx: &egui::Context,
        events: Vec<egui::Event>,
    ) -> Vec<(String, egui::Rect)> {
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(980.0, 680.0),
            )),
            events,
            ..Default::default()
        };
        let mut output = ctx.run_ui(raw, |ui| {
            app.show_window(ui);
            if app.delete_asked_in_window {
                app.show_confirm_delete_modal(ui.ctx());
            }
        });
        output.textures_delta.clear();
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
        for clipped in &output.shapes {
            walk(&clipped.shape, &mut into);
        }
        into
    }

    fn click_with_modal(app: &mut App, ctx: &egui::Context, at: egui::Pos2) {
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
            painted_with_modal(app, ctx, events);
        }
    }

    #[test]
    fn a_model_is_deleted_from_the_main_window_after_a_question_asked_there() {
        let dir = scratch("eb-delete");
        let mut app = app_in(&dir, "llamacpp");
        with_models(&mut app, &dir);
        let ctx = egui::Context::default();

        // what may be deleted: the person's files, not what came with the application
        let shipped = index_of(&app, "shipped-Q4_K_M.gguf");
        assert!(app.delete_request(shipped).is_err());
        let alpha = index_of(&app, "alpha-Q4_K_M.gguf");
        assert!(app.delete_request(alpha).is_ok());

        // the button beside the name asks, in this window
        app.selected = alpha;
        let mut found = Vec::new();
        for _ in 0..4 {
            found = painted_with_modal(&mut app, &ctx, Vec::new());
        }
        let button = found
            .iter()
            .find(|(t, _)| t == "Delete…")
            .map(|(_, r)| *r)
            .expect("a Delete… button by the name");
        click_with_modal(&mut app, &ctx, button.center());
        assert!(app.pending_delete.is_some() && app.delete_asked_in_window);
        let mut found = Vec::new();
        for _ in 0..3 {
            found = painted_with_modal(&mut app, &ctx, Vec::new());
        }
        assert!(
            found.iter().any(|(t, _)| t == "Delete model?"),
            "the question is not asked: {found:?}"
        );
        // closing Settings meanwhile does not answer it
        app.close_settings();
        assert!(app.pending_delete.is_some());
        let yes = found
            .iter()
            .filter(|(t, _)| t == "Delete")
            .map(|(_, r)| *r)
            .next_back()
            .expect("the button that says yes");
        click_with_modal(&mut app, &ctx, yes.center());
        assert!(app.pending_delete.is_none() && !app.delete_asked_in_window);
        assert!(
            !dir.join("models/alpha-Q4_K_M.gguf").exists(),
            "the file is still there"
        );
        assert!(app.models.iter().all(|m| m.filename != "alpha-Q4_K_M.gguf"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    #[cfg(unix)]
    fn another_model_chosen_while_llamacpp_runs_restarts_it_with_that_model_and_the_one_running_is_not_deleted(
    ) {
        let dir = scratch("eb-switch-model");
        let mut app = app_in(&dir, "llamacpp");
        with_models(&mut app, &dir);
        app.binary = crate::testkit::procs::write_script(&dir, "llama-server", "exec sleep 30");
        let ctx = egui::Context::default();
        let alpha = index_of(&app, "alpha-Q4_K_M.gguf");
        let beta = index_of(&app, "beta-Q4_K_M.gguf");

        // alpha runs (a plan with nothing to wait for is running at once)
        app.selected = alpha;
        let model = app.models[alpha].clone();
        let selection = crate::launch::selection_from_values(
            "llamacpp",
            Some(model.info()),
            app.values.map(),
            false,
        );
        let mut plan = crate::launch::plan_selection(
            &app.registry,
            &selection,
            &app.binary,
            app.machine.clone(),
            None,
        )
        .unwrap_or_else(|e| panic!("{e:#?}"));
        plan.units[0].ready = None;
        app.session.start(&plan, &model.display, || {});
        until("alpha to run", || {
            app.session.poll(&mut |_| {}, &mut |_| {});
            app.session.state() == ServerState::Running
        });
        assert!(
            !app.can_switch_selected(),
            "the model that runs is the one chosen"
        );
        assert!(
            app.delete_request(alpha).is_err(),
            "the model that runs is not deleted"
        );

        // beta is chosen: the button starts the server again with it
        app.selected = beta;
        assert!(app.can_switch_selected());
        let found = settled(&mut app, &ctx, 980.0);
        assert!(in_toolbar(&found, "Use this model").is_some(), "{found:?}");
        app.switch_to_selected();
        let mut log = Vec::new();
        until("the server to come back with beta", || {
            app.session.poll(&mut |line| log.push(line), &mut |_| {});
            app.poll_restart(&ctx);
            app.session.model_name() == Some(app.models[beta].display.as_str())
        });
        assert!(!app.restart_with_selected);
        until("the command line of the new run", || {
            app.session.poll(&mut |line| log.push(line), &mut |_| {});
            log.iter()
                .any(|l| l.starts_with("$ ") && l.contains("beta-Q4_K_M.gguf"))
        });
        app.session.stop_blocking();
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn changing_the_engine_is_not_a_change_of_the_default_which_is_the_engine_in_use_at_its_defaults(
    ) {
        let dir = scratch("eb-default");
        let mut app = app_in(&dir, "llamacpp");
        assert!(!app.configuration_modified());
        app.switch_engine("ollama");
        assert_eq!(app.engine, "ollama");
        assert!(
            !app.configuration_modified(),
            "{:?}",
            app.changes_since_saved()
        );
        // a setting changed is a change, and the Default brings the engine's own back without leaving it
        app.values.set("port", 12000u16);
        assert!(app.configuration_modified());
        app.choose_configuration("default");
        assert_eq!(app.engine, "ollama");
        assert_ne!(
            app.values.int("port"),
            Some(12000),
            "the engine's own port again"
        );
        assert!(!app.configuration_modified());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_long_list_of_models_has_a_box_that_narrows_it_and_a_short_one_does_not() {
        let dir = scratch("eb-filter");
        let mut app = app_in(&dir, "llamacpp");
        let folder = dir.join("models");
        std::fs::create_dir_all(&folder).unwrap();
        let ctx = egui::Context::default();
        let names: Vec<String> = (0..10).map(|i| format!("model-{i}-Q4_K_M.gguf")).collect();
        for name in names.iter().take(3) {
            std::fs::File::create(folder.join(name))
                .unwrap()
                .set_len(1024)
                .unwrap();
        }
        app.models_dir = folder.clone();
        app.models = app.scan_models();
        let in_list = |found: &[(String, egui::Rect)], text: &str| {
            found
                .iter()
                .any(|(t, r)| t.contains(text) && r.center().x < 210.0)
        };
        let found = settled(&mut app, &ctx, 980.0);
        assert!(
            !found.iter().any(|(t, _)| t == "Filter models..."),
            "three models need no box"
        );

        for name in names.iter().skip(3) {
            std::fs::File::create(folder.join(name))
                .unwrap()
                .set_len(1024)
                .unwrap();
        }
        app.models = app.scan_models();
        let found = settled(&mut app, &ctx, 980.0);
        assert!(
            found.iter().any(|(t, _)| t == "Filter models..."),
            "ten models have a box"
        );
        app.model_filter = "model-7".into();
        let found = settled(&mut app, &ctx, 980.0);
        assert!(in_list(&found, "model-7-Q4_K_M"), "{found:?}");
        assert!(
            !in_list(&found, "model-3-Q4_K_M"),
            "a model the words do not name is listed"
        );
        app.model_filter = "nothing like it".into();
        let found = settled(&mut app, &ctx, 980.0);
        assert!(found.iter().any(|(t, _)| t == "No model matches."));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
