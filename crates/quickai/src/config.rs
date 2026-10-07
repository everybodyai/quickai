//! Where QuickAI's documents come from, and the registry built from them
//! (docs/architecture.md, section 5.2). The core merges and checks; this module is the effectful
//! part: the documents compiled into the application, the ones beside the executable, the user's
//! own, and the override directory tests point at.
//!
//! Layers, lowest first: embedded, bundle (`<exe dir>/config`), user (`~/.config/quickai`),
//! override (`QUICKAI_CONFIG_DIR`). `QUICKAI_SAFE=1` ignores the user layer.

use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry};
use quickai_core::config::{load, Layer, LayerSource, Listing, LoadOptions, SourceDoc};
use quickai_core::diag::Diagnostic;
use quickai_core::doc::Kind;
use quickai_core::registry::{KnownPrimitives, Registry};

use crate::prims;

static EMBEDDED: Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/manifests");

/// The documents compiled into the application.
pub struct EmbeddedSource;

impl LayerSource for EmbeddedSource {
    fn read(&self) -> Listing {
        fn walk(dir: &Dir<'static>, out: &mut Vec<SourceDoc>) {
            for entry in dir.entries() {
                match entry {
                    DirEntry::Dir(d) => walk(d, out),
                    DirEntry::File(f) if f.path().extension().is_some_and(|e| e == "toml") => {
                        out.push(SourceDoc {
                            name: format!(
                                "embedded:{}",
                                f.path().display().to_string().replace('\\', "/")
                            ),
                            text: f.contents_utf8().unwrap_or_default().to_string(),
                        });
                    }
                    DirEntry::File(_) => {}
                }
            }
        }
        let mut docs = Vec::new();
        walk(&EMBEDDED, &mut docs);
        docs.sort_by(|a, b| a.name.cmp(&b.name));
        Listing {
            docs,
            problems: Vec::new(),
        }
    }
}

/// A layer kept in a directory: `<root>/engines/*.toml`, `<root>/vocab/*.toml` and so on.
pub struct FsSource {
    pub root: PathBuf,
}

impl LayerSource for FsSource {
    fn read(&self) -> Listing {
        let mut listing = Listing::default();
        for kind in Kind::ALL {
            let dir = self.root.join(kind.dir());
            let entries = match std::fs::read_dir(&dir) {
                Ok(entries) => entries,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => continue,
                Err(e) => {
                    listing.problems.push(
                        Diagnostic::warning(format!("could not read the directory: {e}"))
                            .in_file(dir.display().to_string()),
                    );
                    continue;
                }
            };
            let mut paths: Vec<PathBuf> = entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.is_file() && p.extension().is_some_and(|e| e == "toml"))
                .collect();
            paths.sort();
            for path in paths {
                match std::fs::read_to_string(&path) {
                    Ok(text) => listing.docs.push(SourceDoc {
                        name: path.display().to_string(),
                        text,
                    }),
                    Err(e) => listing.problems.push(
                        Diagnostic::error(format!("could not read the file: {e}"))
                            .in_file(path.display().to_string())
                            .with_hint("the document was skipped"),
                    ),
                }
            }
        }
        listing
    }
}

/// The directories of the optional layers, resolved from the environment.
#[derive(Clone, Debug, Default)]
pub struct ConfigDirs {
    pub bundle: Option<PathBuf>,
    pub user: Option<PathBuf>,
    pub override_dir: Option<PathBuf>,
    pub safe_mode: bool,
}

impl ConfigDirs {
    /// `<exe dir>/config`, the user's config directory, `QUICKAI_CONFIG_DIR` and `QUICKAI_SAFE`.
    pub fn from_environment() -> Self {
        let flag = |name: &str| std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0");
        ConfigDirs {
            bundle: std::env::current_exe()
                .ok()
                .and_then(|exe| exe.parent().map(|d| d.join("config"))),
            user: directories::ProjectDirs::from("dev", "quickai", "quickai")
                .map(|d| d.config_dir().to_path_buf()),
            override_dir: std::env::var_os("QUICKAI_CONFIG_DIR")
                .filter(|v| !v.is_empty())
                .map(PathBuf::from),
            safe_mode: flag("QUICKAI_SAFE"),
        }
    }
}

/// Loads every layer and builds the registry. It always returns one: what could not be loaded is
/// in `registry.diagnostics`, and the embedded documents keep the application working.
pub fn build_registry(dirs: &ConfigDirs) -> Registry {
    build_registry_with(dirs, &prims::known())
}

/// `build_registry` for a build that provides `known` (a test of what a build that lacks a primitive
/// does with the documents that need it).
pub fn build_registry_with(dirs: &ConfigDirs, known: &KnownPrimitives) -> Registry {
    let embedded = EmbeddedSource;
    let bundle = dirs.bundle.as_deref().map(root_source);
    let user = dirs.user.as_deref().map(root_source);
    let over = dirs.override_dir.as_deref().map(root_source);

    let mut layers: Vec<(Layer, &dyn LayerSource)> = vec![(Layer::Embedded, &embedded)];
    if let Some(s) = &bundle {
        layers.push((Layer::Bundle, s));
    }
    if let Some(s) = &user {
        layers.push((Layer::User, s));
    }
    if let Some(s) = &over {
        layers.push((Layer::Override, s));
    }
    let loaded = load(
        &layers,
        &LoadOptions {
            safe_mode: dirs.safe_mode,
            app_version: Some(env!("CARGO_PKG_VERSION").to_string()),
        },
    );
    Registry::build(loaded, known)
}

/// The engine to run: the one the settings name, or llama.cpp when this build has no document for
/// it (an engine of a newer version, a document that did not load), with a sentence for the log.
/// What the settings hold for the engine that is not used is left alone, and so is the choice: it is
/// not forgotten, and comes back when the document does.
pub fn engine_in_use(
    registry: &Registry,
    settings: &crate::settings::Settings,
) -> (String, Option<String>) {
    match settings.chosen_engine() {
        Some(wanted) if registry.engine(&wanted).is_some() => (wanted, None),
        Some(wanted) => (
            crate::settings::DEFAULT_ENGINE.to_string(),
            Some(format!(
                "config: the settings name the engine `{wanted}`, which this build has no document for; \
                 using llama.cpp. What is stored for `{wanted}` is kept."
            )),
        ),
        None => (crate::settings::DEFAULT_ENGINE.to_string(), None),
    }
}

fn root_source(root: &Path) -> FsSource {
    FsSource {
        root: root.to_path_buf(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The form of the embedded engine in four situations, as JSON: what the window shows, which
    /// fields are visible and enabled, where each value came from. A change to the document or to
    /// the form model shows up here as a diff to read (`QUICKAI_UPDATE_GOLDEN=1` rewrites it).
    #[test]
    fn the_embedded_engines_form_in_four_states_matches_the_golden_file() {
        use quickai_core::form::FormModel;
        use quickai_core::plan::{HostFacts, ModelInfo, Selection};

        const GB: u64 = 1024 * 1024 * 1024;
        let registry = build_registry(&ConfigDirs::default());
        let engine = registry.engine("llamacpp").expect("the embedded engine");
        let facts = |ram_gb: u64, running: bool| HostFacts {
            os: "linux".into(),
            arch: "x86_64".into(),
            ram_bytes: ram_gb * GB,
            cpus: 8,
            binary: Some("/opt/quickai/llama-server".into()),
            session_running: running,
            ..Default::default()
        };
        let model = |size_gb: u64| ModelInfo {
            id: "m.gguf".into(),
            display: "M".into(),
            path: "/models/m.gguf".into(),
            size_bytes: size_gb * GB,
        };
        // what a first run holds (the context size is a stored 4096), and what is held when nothing is
        // stored yet, which lets the context size follow the model
        let first_run = crate::settings::Settings::default().engine_values(engine);
        let mut nothing_stored = first_run.clone();
        nothing_stored.remove("llamacpp.ctx_size");
        let form = |values: &std::collections::BTreeMap<String, quickai_core::value::Value>,
                    model: Option<ModelInfo>,
                    facts: &HostFacts| {
            let mut selection = Selection::new("llamacpp");
            selection.model = model;
            selection.values = values.clone();
            serde_json::to_value(FormModel::build(&registry, &selection, facts).unwrap()).unwrap()
        };
        let golden = serde_json::json!({
            "first_run_no_model": form(&first_run, None, &facts(16, false)),
            "small_model_default_context": form(&nothing_stored, Some(model(1)), &facts(16, false)),
            "large_model_little_ram": form(&nothing_stored, Some(model(20)), &facts(8, false)),
            "server_running": form(&first_run, Some(model(1)), &facts(16, true)),
        });
        crate::golden::assert_golden(
            "form_llamacpp.json",
            &(serde_json::to_string_pretty(&golden).unwrap() + "\n"),
        );
    }

    /// The labels, hover texts and hints moved out of the window's code into the engine document.
    /// `tests/golden/ui_texts.json` was extracted from the code before it was deleted, so this fails
    /// if a word of what the user reads (and the GUI suite's OCR looks for) has changed on the way.
    #[test]
    fn the_engine_documents_texts_are_the_texts_the_window_always_had() {
        let registry = build_registry(&ConfigDirs::default());
        let engine = registry.engine("llamacpp").expect("the embedded engine");
        let golden: std::collections::BTreeMap<String, String> =
            serde_json::from_str(&crate::golden::read_golden("ui_texts.json")).unwrap();
        assert!(
            golden.len() >= 14,
            "the golden file lists every text: {}",
            golden.len()
        );
        for (key, want) in &golden {
            // the runtime manager's sentences are checked where they are built (app/runtime_manager.rs)
            if key.starts_with("runtime.") {
                continue;
            }
            let (id, what) = key.split_once('.').expect("field.kind");
            let field = engine
                .doc
                .fields
                .iter()
                .find(|f| f.id == id)
                .unwrap_or_else(|| panic!("the document has no field `{id}`"));
            let got = match what {
                // the window puts the colon after a label that sits in front of its control
                "label" => format!("{}:", field.label.as_deref().unwrap_or_default()),
                "help" => field.help.clone().unwrap_or_default(),
                "hint" => field.ui.hint.clone().unwrap_or_default(),
                other => panic!("`{other}` is not a kind of text"),
            };
            assert_eq!(&got, want, "{key}");
        }
    }
    use quickai_core::diag::Severity;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-config-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn none() -> ConfigDirs {
        ConfigDirs::default()
    }

    /// The embedded documents are part of the program: a mistake in one is a bug, found by this
    /// test rather than by a user (docs/architecture.md, section 5.3).
    #[test]
    fn the_embedded_documents_are_valid_and_complain_about_nothing() {
        let registry = build_registry(&none());
        assert!(
            registry.diagnostics.is_empty(),
            "the embedded documents have problems:\n{}",
            registry
                .diagnostics
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join("\n")
        );
        let engine = registry.engine("llamacpp").expect("llama.cpp is embedded");
        assert_eq!(engine.doc.engine.name, "llama.cpp");
        assert!(engine.spawn.is_some() && engine.ready.is_some());
    }

    #[test]
    fn every_primitive_the_embedded_documents_need_is_provided() {
        let registry = build_registry(&none());
        for engine in registry.engines() {
            for needed in &engine.merged.envelope.requires_primitives {
                assert!(
                    prims::known().supports(needed),
                    "{} needs {needed}, which `prims::PROVIDED` lacks",
                    engine.id
                );
            }
        }
    }

    /// `settings.json` has nine keys the GUI suite and users' files depend on. Seven belong to the
    /// engine's fields (each names its key with `legacy_key`); the other two, `dark_mode` and
    /// `enable_traffic_proxy`, are the application's own until the `ui` document exists.
    #[test]
    fn the_engines_legacy_keys_and_the_apps_own_are_the_nine_settings_json_keys() {
        let registry = build_registry(&none());
        let engine = registry.engine("llamacpp").unwrap();
        let mut keys: Vec<&str> = engine
            .doc
            .fields
            .iter()
            .filter_map(|f| f.legacy_key.as_deref())
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec![
                "ctx_size",
                "extra_args",
                "last_model_filename",
                "llama_bin_override",
                "models_dir_override",
                "n_gpu_layers",
                "port"
            ]
        );
        keys.extend(["dark_mode", "enable_traffic_proxy"]);
        keys.sort_unstable();
        let mut nine: Vec<String> = serde_json::to_value(crate::settings::Settings::default())
            .unwrap()
            .as_object()
            .unwrap()
            .keys()
            .cloned()
            .collect();
        nine.sort_unstable();
        assert_eq!(keys, nine, "every key of the file is accounted for");
    }

    #[test]
    fn a_user_document_patches_the_embedded_one() {
        let user = scratch("user");
        std::fs::create_dir_all(user.join("engines")).unwrap();
        std::fs::write(
            user.join("engines/llamacpp.toml"),
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[[field]]\nid = \"n_gpu_layers\"\ndefault = 20\n",
        )
        .unwrap();
        let registry = build_registry(&ConfigDirs {
            user: Some(user.clone()),
            ..none()
        });
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        let engine = registry.engine("llamacpp").unwrap();
        assert_eq!(engine.merged.contributors.len(), 2);
        assert!(engine.merged.contributors[1].file.contains("llamacpp.toml"));
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn a_broken_user_document_leaves_the_embedded_one_in_force() {
        let user = scratch("broken");
        std::fs::create_dir_all(user.join("engines")).unwrap();
        std::fs::write(
            user.join("engines/llamacpp.toml"),
            "schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[launch.spawn]\nargs = [\"${modl.main.path}\"]\n",
        )
        .unwrap();
        let registry = build_registry(&ConfigDirs {
            user: Some(user.clone()),
            ..none()
        });
        assert!(registry.engine("llamacpp").is_some());
        let errors: Vec<String> = registry
            .diagnostics
            .iter()
            .filter(|d| d.severity == Severity::Error)
            .map(|d| d.to_string())
            .collect();
        assert_eq!(errors.len(), 1, "{errors:?}");
        assert!(errors[0].contains("llamacpp.toml:5"), "{}", errors[0]);
        assert!(errors[0].contains("unknown name `modl`"), "{}", errors[0]);
        assert!(errors[0].contains("did you mean `model`?"), "{}", errors[0]);
        let _ = std::fs::remove_dir_all(&user);
    }

    #[test]
    fn safe_mode_ignores_the_user_directory_and_the_override_still_applies() {
        let user = scratch("safe-user");
        let over = scratch("safe-over");
        for (dir, port) in [(&user, "1"), (&over, "2")] {
            std::fs::create_dir_all(dir.join("engines")).unwrap();
            std::fs::write(
                dir.join("engines/llamacpp.toml"),
                format!("schema = 1\nkind = \"engine\"\nid = \"llamacpp\"\n[provides]\ndefault_port = {port}\n"),
            )
            .unwrap();
        }
        let registry = build_registry(&ConfigDirs {
            user: Some(user.clone()),
            override_dir: Some(over.clone()),
            safe_mode: true,
            ..none()
        });
        let engine = registry.engine("llamacpp").unwrap();
        assert_eq!(engine.doc.provides.default_port, Some(2));
        assert_eq!(
            engine.merged.contributors.len(),
            2,
            "embedded and override only"
        );
        assert!(registry
            .diagnostics
            .iter()
            .any(|d| d.message.contains("safe mode")));
        let _ = std::fs::remove_dir_all(&user);
        let _ = std::fs::remove_dir_all(&over);
    }

    #[test]
    fn missing_directories_are_normal_and_unreadable_files_are_reported() {
        let dir = scratch("fs");
        // nothing there yet: not a problem
        assert!(FsSource {
            root: dir.join("absent")
        }
        .read()
        .problems
        .is_empty());
        std::fs::create_dir_all(dir.join("engines")).unwrap();
        std::fs::write(dir.join("engines/not-utf8.toml"), [0xff, 0xfe, 0xfd]).unwrap();
        std::fs::write(dir.join("engines/readme.txt"), "ignored").unwrap();
        std::fs::write(dir.join("engines/b.toml"), "x = 1").unwrap();
        std::fs::write(dir.join("engines/a.toml"), "x = 2").unwrap();
        let listing = FsSource { root: dir.clone() }.read();
        let names: Vec<String> = listing
            .docs
            .iter()
            .map(|d| {
                Path::new(&d.name)
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect();
        assert_eq!(names, vec!["a.toml", "b.toml"], "sorted, and only .toml");
        assert_eq!(listing.problems.len(), 1);
        assert!(listing.problems[0].to_string().contains("not-utf8.toml"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_environment_decides_the_directories() {
        let dirs = ConfigDirs::from_environment();
        // bundle and user directories are derived from this machine (and its conventions: the user
        // directory is `~/.config/quickai` on Linux, `%APPDATA%` on Windows, `~/Library` on macOS);
        // their existence is not required
        assert!(dirs.bundle.is_some());
        assert!(dirs.user.is_some());
    }
}
