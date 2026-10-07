//! From what the user picked to a run plan: the glue between the interface's state and the planner.
//!
//! The interface keeps its settings as plain fields; a run is a `Selection` of those, facts about
//! this machine, and the engine's document. Nothing here decides *what* a command line looks like
//! (the document does) or *how* it is carried out (`runtime::Session` does).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quickai_core::diag::Diagnostic;
use quickai_core::plan::{plan, HostFacts, ModelInfo, RunPlan, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::registry::Registry;
use quickai_core::value::Value;

use crate::gpu::GpuVendor;
#[cfg(test)]
use crate::models::ModelEntry;
use crate::paths;
use crate::prims::locate::{locate, LocateEnv};
#[cfg(test)]
use crate::settings::DEFAULT_ENGINE;

/// What the settings controls hold for a run (the way the tests of the session describe llama.cpp's).
#[cfg(test)]
#[derive(Clone, Debug)]
pub struct RunSettings {
    pub port: u16,
    pub ctx_size: u32,
    pub n_gpu_layers: u32,
    pub extra_args: String,
    /// A custom executable the user chose, as they typed it.
    pub bin_override: Option<String>,
    pub traffic_proxy: bool,
}

/// What is known about the machine, gathered once when the application starts.
#[derive(Clone, Debug, Default)]
pub struct MachineFacts {
    pub ram_bytes: u64,
    /// Lower-case vendor names (`nvidia`, `amd`, `intel`, `apple`).
    pub gpu_vendors: Vec<String>,
    /// False when nothing could be detected.
    pub gpu_known: bool,
}

impl MachineFacts {
    pub fn detect(ram_bytes: u64, gpus: &[GpuVendor]) -> Self {
        MachineFacts {
            ram_bytes,
            gpu_vendors: gpus
                .iter()
                .map(|g| g.label().to_ascii_lowercase())
                .collect(),
            gpu_known: !gpus.is_empty(),
        }
    }
}

/// What the window asks the planner about: the engine, the model (if one is selected), the values of
/// the engine's fields as the window holds them, and whether the live-traffic proxy is wanted.
pub fn selection_from_values(
    engine: &str,
    model: Option<ModelInfo>,
    values: &BTreeMap<String, Value>,
    traffic_proxy: bool,
) -> Selection {
    let mut selection = Selection::new(engine);
    selection.model = model;
    selection.values = values.clone();
    selection
        .values
        .insert(TRAFFIC_PROXY_KEY.to_string(), Value::Bool(traffic_proxy));
    selection
}

/// The selection the tests of the session have always built for llama.cpp from `RunSettings`.
#[cfg(test)]
pub fn selection(model: &ModelEntry, settings: &RunSettings) -> Selection {
    let mut selection = Selection::new(DEFAULT_ENGINE)
        .with_model(model.info())
        .set("llamacpp.port", settings.port)
        .set("llamacpp.ctx_size", settings.ctx_size)
        .set("llamacpp.n_gpu_layers", settings.n_gpu_layers)
        .set("llamacpp.extra_args", settings.extra_args.as_str())
        .set(TRAFFIC_PROXY_KEY, settings.traffic_proxy);
    if let Some(path) = &settings.bin_override {
        selection = selection.set("llamacpp.bin_override", path.as_str());
    }
    selection
}

pub fn host_facts(binary: &Path, machine: MachineFacts, internal_port: Option<u16>) -> HostFacts {
    let home = directories::BaseDirs::new()
        .map(|d| d.home_dir().to_path_buf())
        .unwrap_or_else(std::env::temp_dir);
    HostFacts {
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        ram_bytes: machine.ram_bytes,
        cpus: std::thread::available_parallelism().map_or(1, |n| n.get() as u32),
        gpu_known: machine.gpu_known,
        gpu_vendors: machine.gpu_vendors,
        binary: Some(binary.display().to_string()),
        data_dir: home.join(".quickai").display().to_string(),
        config_dir: directories::ProjectDirs::from("dev", "quickai", "quickai")
            .map(|d| d.config_dir().display().to_string())
            .unwrap_or_default(),
        exe_dir: std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|p| p.display().to_string()))
            .unwrap_or_default(),
        home: home.display().to_string(),
        internal_port,
        ..Default::default()
    }
}

/// The run plan for starting `model` with `settings`: llama.cpp, as the session's tests describe it.
#[cfg(test)]
pub fn plan_run(
    registry: &Registry,
    model: &ModelEntry,
    settings: &RunSettings,
    binary: &Path,
    machine: MachineFacts,
    internal_port: Option<u16>,
) -> Result<RunPlan, Vec<Diagnostic>> {
    plan_selection(
        registry,
        &selection(model, settings),
        binary,
        machine,
        internal_port,
    )
}

/// The run plan for any selection: the entry point for callers that set only some values and let the
/// fields' defaults fill in the rest (the headless `plan` command).
pub fn plan_selection(
    registry: &Registry,
    selection: &Selection,
    binary: &Path,
    machine: MachineFacts,
    internal_port: Option<u16>,
) -> Result<RunPlan, Vec<Diagnostic>> {
    plan(
        registry,
        selection,
        &host_facts(binary, machine, internal_port),
    )
}

/// Where the engine's executable is: the user's choice if there is one, otherwise the first of the
/// document's other strategies that finds it. When nothing exists the last candidate is returned
/// anyway, so there is a path to name in "not found at ...".
pub fn locate_binary(registry: &Registry, engine_id: &str, bin_override: Option<&str>) -> PathBuf {
    let Some(engine) = registry.engine(engine_id) else {
        return PathBuf::from(DEFAULT_PROGRAM);
    };
    let program = program_name(engine);
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let dev_root = paths::dev_repo_root();
    let setting = |field: &str| {
        (field == "bin_override")
            .then(|| bin_override.map(str::to_string))
            .flatten()
    };
    let facts = host_facts(Path::new(""), MachineFacts::default(), None);
    locate(
        &engine.locate,
        &LocateEnv {
            exe_dir: exe_dir.as_deref(),
            dev_root: &dev_root,
            setting: &setting,
            facts: &facts,
            var: &|name| std::env::var(name).ok(),
        },
    )
    .map(|found| found.path)
    .unwrap_or_else(|| PathBuf::from(program))
}

/// The program a missing engine is named by when nothing else says.
const DEFAULT_PROGRAM: &str = "llama-server";

/// What the engine's program is called, for "not found at ...": the name the document looks for on
/// the `PATH` or beside the application, else its server's name in lower case.
fn program_name(engine: &quickai_core::registry::Engine) -> String {
    use quickai_core::doc::engine::LocateStrategy;
    engine
        .doc
        .locate
        .strategies
        .iter()
        .find_map(|strategy| match strategy {
            LocateStrategy::PathSearch { name } => Some(name.clone()),
            LocateStrategy::NextToExe { file } => Some(file.clone()),
            _ => None,
        })
        .unwrap_or_else(|| {
            engine
                .doc
                .engine
                .server_name
                .clone()
                .unwrap_or_else(|| engine.doc.engine.name.clone())
                .to_ascii_lowercase()
        })
}

/// The executable ignoring any override: what "Reset to default" returns to.
pub fn default_binary(registry: &Registry, engine_id: &str) -> PathBuf {
    locate_binary(registry, engine_id, None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{build_registry, ConfigDirs};

    fn model() -> ModelEntry {
        ModelEntry {
            filename: "m.gguf".into(),
            path: PathBuf::from("/models/m.gguf"),
            display: "M".into(),
            note: String::new(),
            bundled: false,
            size_bytes: 123,
        }
    }

    fn settings() -> RunSettings {
        RunSettings {
            port: 8080,
            ctx_size: 4096,
            n_gpu_layers: 999,
            extra_args: "--threads 8".into(),
            bin_override: None,
            traffic_proxy: false,
        }
    }

    #[test]
    fn the_selection_carries_every_setting_under_the_engines_field_ids() {
        let s = selection(&model(), &settings());
        assert_eq!(s.engine, "llamacpp");
        assert_eq!(s.values["llamacpp.port"].as_i64(), Some(8080));
        assert_eq!(s.values["llamacpp.ctx_size"].as_i64(), Some(4096));
        assert_eq!(s.values["llamacpp.n_gpu_layers"].as_i64(), Some(999));
        assert_eq!(
            s.values["llamacpp.extra_args"].as_str(),
            Some("--threads 8")
        );
        assert_eq!(s.values[TRAFFIC_PROXY_KEY].as_bool(), Some(false));
        assert!(!s.values.contains_key("llamacpp.bin_override"));
        assert_eq!(s.model.as_ref().unwrap().size_bytes, 123);

        let mut with_override = settings();
        with_override.bin_override = Some("/opt/llama/llama-server".into());
        let s = selection(&model(), &with_override);
        assert_eq!(
            s.values["llamacpp.bin_override"].as_str(),
            Some("/opt/llama/llama-server")
        );
    }

    #[test]
    fn every_selection_key_is_a_field_of_the_embedded_engine() {
        let registry = build_registry(&ConfigDirs::default());
        let engine = registry.engine(DEFAULT_ENGINE).unwrap();
        let mut with_override = settings();
        with_override.bin_override = Some("/x".into());
        for key in selection(&model(), &with_override).values.keys() {
            if let Some(field) = key.strip_prefix("llamacpp.") {
                assert!(
                    engine.fields.iter().any(|f| f.id == field),
                    "{key} is not a field"
                );
            }
        }
    }

    #[test]
    fn a_user_chosen_binary_wins_even_when_missing_and_default_ignores_it() {
        let registry = build_registry(&ConfigDirs::default());
        assert_eq!(
            locate_binary(&registry, "llamacpp", Some("/nowhere/llama-server")),
            PathBuf::from("/nowhere/llama-server")
        );
        assert_eq!(
            locate_binary(&registry, "llamacpp", Some("  ")),
            default_binary(&registry, "llamacpp")
        );
        let default = default_binary(&registry, "llamacpp");
        let name = default.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("llama-server"), "{name}");
    }

    /// The window asks the planner with the values it holds (`selection_from_values`); the tests of the
    /// session have always asked with `RunSettings`. Both must give llama.cpp the same plan, whatever
    /// the settings, which is the guarantee that choosing an engine did not change llama.cpp's run.
    #[test]
    fn what_the_window_asks_the_planner_is_what_the_tests_of_the_session_always_asked() {
        use crate::settings::Settings;
        let registry = build_registry(&ConfigDirs::default());
        let engine = registry.engine("llamacpp").unwrap();
        let cases = [
            settings(),
            RunSettings {
                port: 9000,
                ctx_size: 2048,
                n_gpu_layers: 0,
                extra_args: String::new(),
                bin_override: None,
                traffic_proxy: false,
            },
            RunSettings {
                port: 8080,
                ctx_size: 131072,
                n_gpu_layers: 20,
                extra_args: "--threads 8 --flash-attn on --foo \"a b\"".into(),
                bin_override: Some("/opt/llama/llama-server".into()),
                traffic_proxy: true,
            },
        ];
        for run in cases {
            // the file as the window would have saved these settings, read back as the window holds them
            let file = Settings {
                port: run.port,
                ctx_size: run.ctx_size,
                n_gpu_layers: run.n_gpu_layers,
                extra_args: run.extra_args.clone(),
                llama_bin_override: run.bin_override.clone(),
                last_model_filename: Some("m.gguf".into()),
                models_dir_override: Some("/data/models".into()),
                ..Settings::default()
            };
            let held = file.engine_values(engine);
            let from_values =
                selection_from_values("llamacpp", Some(model().info()), &held, run.traffic_proxy);
            let binary = Path::new("/opt/quickai/llama-server");
            let internal = run.traffic_proxy.then_some(41234);
            let one = plan_selection(
                &registry,
                &from_values,
                binary,
                MachineFacts::default(),
                internal,
            );
            let other = plan_run(
                &registry,
                &model(),
                &run,
                binary,
                MachineFacts::default(),
                internal,
            );
            assert_eq!(
                serde_json::to_value(one.unwrap()).unwrap(),
                serde_json::to_value(other.unwrap()).unwrap(),
                "{run:?}"
            );
        }
    }

    #[test]
    fn a_plan_comes_out_for_the_embedded_engine() {
        let registry = build_registry(&ConfigDirs::default());
        let plan = plan_run(
            &registry,
            &model(),
            &settings(),
            Path::new("/opt/quickai/llama-server"),
            MachineFacts::default(),
            None,
        )
        .unwrap();
        assert_eq!(plan.engine, "llamacpp");
        assert_eq!(plan.public_port, Some(8080));
    }

    #[test]
    fn machine_facts_name_vendors_in_lower_case() {
        let facts = MachineFacts::detect(1024, &[GpuVendor::Nvidia, GpuVendor::Intel]);
        assert_eq!(facts.gpu_vendors, vec!["nvidia", "intel"]);
        assert!(facts.gpu_known);
        assert!(!MachineFacts::detect(1, &[]).gpu_known);
    }
}
