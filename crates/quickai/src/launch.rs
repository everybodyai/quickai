//! From what the user picked to a run plan: the glue between the interface's state and the planner.
//!
//! The interface keeps its settings as plain fields; a run is a `Selection` of those, facts about
//! this machine, and the engine's document. Nothing here decides *what* a command line looks like
//! (the document does) or *how* it is carried out (`runtime::Session` does).

use std::path::{Path, PathBuf};

use quickai_core::diag::Diagnostic;
use quickai_core::plan::{plan, HostFacts, ModelInfo, RunPlan, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::registry::Registry;

use crate::gpu::GpuVendor;
use crate::models::ModelEntry;
use crate::paths;
use crate::prims::locate::{locate, LocateEnv};

/// The one engine this version of the interface offers. Choosing an engine arrives with the second.
pub const ENGINE_ID: &str = "llamacpp";

/// What the settings controls hold for a run.
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

pub fn selection(model: &ModelEntry, settings: &RunSettings) -> Selection {
    let mut selection = Selection::new(ENGINE_ID)
        .with_model(ModelInfo {
            id: model.filename.clone(),
            display: model.display.clone(),
            path: model.path.display().to_string(),
            size_bytes: model.size_bytes,
        })
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

/// The run plan for starting `model` with `settings`. `internal_port` is a free port for the engine
/// to listen on when the live-traffic proxy is on (the caller allocates it, and reports a failure to
/// do so); it is ignored otherwise.
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
pub fn locate_binary(registry: &Registry, bin_override: Option<&str>) -> PathBuf {
    let Some(engine) = registry.engine(ENGINE_ID) else {
        return PathBuf::from("llama-server");
    };
    let exe_dir = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf));
    let dev_root = paths::dev_repo_root();
    let setting = |field: &str| {
        (field == "bin_override")
            .then(|| bin_override.map(str::to_string))
            .flatten()
    };
    locate(
        &engine.locate,
        &LocateEnv {
            exe_dir: exe_dir.as_deref(),
            dev_root: &dev_root,
            setting: &setting,
        },
    )
    .map(|found| found.path)
    .unwrap_or_else(|| PathBuf::from("llama-server"))
}

/// The executable ignoring any override: what "Reset to default" returns to.
pub fn default_binary(registry: &Registry) -> PathBuf {
    locate_binary(registry, None)
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
        let engine = registry.engine(ENGINE_ID).unwrap();
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
            locate_binary(&registry, Some("/nowhere/llama-server")),
            PathBuf::from("/nowhere/llama-server")
        );
        assert_eq!(
            locate_binary(&registry, Some("  ")),
            default_binary(&registry)
        );
        let default = default_binary(&registry);
        let name = default.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("llama-server"), "{name}");
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
