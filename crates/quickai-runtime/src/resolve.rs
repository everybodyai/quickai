//! From what a person asked for to the plan of a run, in one function (docs/headless.md, section 4).
//!
//! The window decides what to run from the controls it holds; a command line decides it from a
//! configuration and a few words. Both end in the same question: which engine, with which model and
//! which values, started from which program on this computer, and what does the planner make of that?
//! It is answered here, once, so that `quickai plan` prints the plan that `quickai-run` runs and neither
//! can drift from the other.
//!
//! Nothing here starts a program or fetches a file. A model that the models folders do not hold is an
//! answer (`ResolveError::ModelNotHere`); getting it is `provision`'s.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use quickai_core::diag::Diagnostic;
use quickai_core::plan::{ModelInfo, RunPlan, Selection, TRAFFIC_PROXY_KEY};
use quickai_core::registry::Registry;
use quickai_core::value::Value;

use crate::config::ConfigDirs;
use crate::launch::{self, MachineFacts};
use crate::models::ModelEntry;

/// What a run is asked for, before the computer is looked at.
#[derive(Clone, Debug, Default)]
pub struct Wanted {
    pub engine: String,
    /// A file on the disk, wherever it is (`quickai plan --model FILE`); for an engine that lists its
    /// models by name, a name.
    pub model_path: Option<String>,
    /// The model a configuration names: a name the engine lists, or the name of a file in the models
    /// folders. Used when `model_path` is not.
    pub model_name: Option<String>,
    /// The values of the engine's fields by `<engine>.<field>`: what a configuration gives and what was
    /// said besides. A field not in it takes the document's default.
    pub values: BTreeMap<String, Value>,
    /// The program, when it is not the one the engine's document finds.
    pub binary: Option<String>,
    /// The live-traffic proxy in front of the engine (only where the engine's traffic is read).
    pub proxy: bool,
    /// The folder a model file is looked for in, when it is not the one the application downloads to.
    pub models_dir: Option<PathBuf>,
}

/// Why a run could not be resolved.
#[derive(Debug)]
pub enum ResolveError {
    /// There is no engine of this name.
    UnknownEngine(String),
    /// The configuration names a model, and the models folders do not hold a file of that name.
    ModelNotHere { name: String },
    /// The engine runs files and the models folders hold none.
    NoModel,
    /// The proxy needed a port of its own and none could be had.
    NoPort(String),
    /// The planner refused, with the problems it found.
    Planner(Vec<Diagnostic>),
}

/// A run that is ready to be started.
#[derive(Debug)]
pub struct Resolved {
    pub plan: RunPlan,
    pub binary: PathBuf,
    /// The model the plan runs; `None` for an engine that runs with none.
    pub model: Option<ModelInfo>,
    pub internal_port: Option<u16>,
}

/// The entry of a model file that is named by its path, as the models folders would list it.
pub fn model_entry(path: &Path) -> ModelEntry {
    let filename = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    ModelEntry {
        display: filename.clone(),
        filename,
        path: path.to_path_buf(),
        note: String::new(),
        bundled: false,
        size_bytes: std::fs::metadata(path).map(|m| m.len()).unwrap_or(0),
        companions: crate::bundles::companions(path),
        mode: crate::bundles::mode(path),
    }
}

/// The folders a model file is looked for in: the ones that come with the application, then the one it
/// downloads to (below `dirs.data` when a test or a person says where the application keeps its data).
pub fn models_dirs(dirs: &ConfigDirs) -> (PathBuf, PathBuf) {
    match &dirs.data {
        Some(data) => (data.join("bundled-models"), data.join("models")),
        None => (
            crate::paths::bundled_models_dir(),
            crate::paths::user_models_dir(),
        ),
    }
}

/// The plan of the run `wanted` asks for on this computer. What is said on the way that is not a failure
/// (a companion file that has gone, a mode the document does not have) is added to `notes`, in the words the
/// log of the window uses.
pub fn resolve(
    registry: &Registry,
    dirs: &ConfigDirs,
    machine: MachineFacts,
    wanted: &Wanted,
    notes: &mut Vec<String>,
) -> Result<Resolved, ResolveError> {
    let engine = wanted.engine.as_str();
    let Some(engine_doc) = registry.engine(engine) else {
        return Err(ResolveError::UnknownEngine(engine.to_string()));
    };
    // an engine that lists its models by name is given one by name (or none: it runs empty)
    let lists_models = engine_doc.models.as_ref().is_some_and(|m| m.list.is_some());
    // one that runs on its own (ComfyUI) is given none: the work a client sends it names the files it uses
    let needs_model = engine_doc.doc.provides.needs_model;
    let model = if !needs_model {
        None
    } else if lists_models {
        let named = wanted
            .model_path
            .clone()
            .or_else(|| wanted.model_name.clone());
        named.map(|name| ModelInfo {
            id: name.clone(),
            display: name,
            path: String::new(),
            size_bytes: 0,
        })
    } else {
        let entry = match &wanted.model_path {
            Some(path) => model_entry(Path::new(path)),
            None => {
                let (bundled, user) = models_dirs(dirs);
                let user = wanted.models_dir.clone().unwrap_or(user);
                let hide = quickai_core::lookup::bundle::companion_patterns(
                    quickai_core::models::bundles(engine_doc),
                );
                let found = crate::models::discover_all(&bundled, &user, &hide);
                // the model a configuration names, which must be here; else the first there is
                match (wanted.model_name.clone(), found.into_iter()) {
                    (Some(name), mut models) => match models.find(|m| m.filename == name) {
                        Some(m) => m,
                        None => return Err(ResolveError::ModelNotHere { name }),
                    },
                    (None, mut models) => match models.next() {
                        Some(m) => m,
                        None => return Err(ResolveError::NoModel),
                    },
                }
            }
        };
        Some(entry.info())
    };

    let mut selection = Selection::new(engine);
    selection.model = model.clone();
    // what the record beside a model's weights adds: its projector, and the mode it is started in
    if needs_model && !lists_models {
        if let Some(path) = selection
            .model
            .as_ref()
            .map(|m| std::path::PathBuf::from(&m.path))
        {
            let (attached, said) =
                launch::attach_record(selection, &model_entry(&path), engine_doc);
            selection = attached;
            notes.extend(said);
        }
    }
    for (key, value) in &wanted.values {
        selection.values.insert(key.clone(), value.clone());
    }
    // the window puts the proxy only in front of an engine whose traffic is read; so does the plan
    let proxy = wanted.proxy && launch::engine_reads_traffic(registry, engine);
    if proxy {
        selection = selection.set(TRAFFIC_PROXY_KEY, true);
    }

    let data = dirs.data.as_ref().map(|d| d.display().to_string());
    let binary =
        launch::locate_binary_in(registry, engine, wanted.binary.as_deref(), data.as_deref());
    let internal_port = if proxy {
        match crate::runtime::process::ephemeral_port() {
            Ok(port) => Some(port),
            Err(e) => return Err(ResolveError::NoPort(e.to_string())),
        }
    } else {
        None
    };
    match launch::plan_selection_in(
        registry,
        &selection,
        &binary,
        machine,
        internal_port,
        data.as_deref(),
    ) {
        Ok(plan) => Ok(Resolved {
            plan,
            binary,
            model,
            internal_port,
        }),
        Err(problems) => Err(ResolveError::Planner(problems)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::build_registry;
    use quickai_core::plan::Start;

    fn world(name: &str) -> (crate::testkit::Scratch, ConfigDirs) {
        let scratch = crate::testkit::scratch(name);
        let dirs = ConfigDirs {
            data: Some(scratch.to_path_buf()),
            ..ConfigDirs::default()
        };
        (scratch, dirs)
    }

    fn spawn_of(resolved: &Resolved) -> (String, Vec<String>) {
        match &resolved.plan.units[0].start[0] {
            Start::Spawn(spawn) => (
                spawn.program.value.clone(),
                spawn.args.iter().map(|a| a.value.clone()).collect(),
            ),
            other => panic!("not a spawn: {other:?}"),
        }
    }

    /// O70: the model a configuration names is the file of that name in the models folder; one that is not
    /// there, and a folder with nothing in it, are answers and not a guess at another model.
    #[test]
    fn p3_o70_the_model_named_is_the_file_of_that_name_and_nothing_else_is_guessed() {
        let (scratch, dirs) = world("o70");
        let registry = build_registry(&ConfigDirs::default());
        std::fs::create_dir_all(scratch.join("models")).unwrap();
        let wanted = |name: Option<&str>| Wanted {
            engine: "llamacpp".into(),
            model_name: name.map(str::to_string),
            ..Wanted::default()
        };
        let mut notes = Vec::new();
        let none = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &wanted(None),
            &mut notes,
        );
        assert!(matches!(none, Err(ResolveError::NoModel)), "{none:?}");

        std::fs::write(scratch.join("models/a-Q4_K_M.gguf"), b"GGUF").unwrap();
        std::fs::write(scratch.join("models/b-Q4_K_M.gguf"), b"GGUF").unwrap();
        let named = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &wanted(Some("b-Q4_K_M.gguf")),
            &mut notes,
        )
        .expect("the file is there");
        assert_eq!(named.model.as_ref().unwrap().display, "b-Q4_K_M.gguf");
        assert_eq!(
            spawn_of(&named).1[1],
            scratch
                .join("models")
                .join("b-Q4_K_M.gguf")
                .display()
                .to_string()
        );

        let gone = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &wanted(Some("c-Q4_K_M.gguf")),
            &mut notes,
        );
        assert!(
            matches!(&gone, Err(ResolveError::ModelNotHere { name }) if name == "c-Q4_K_M.gguf"),
            "{gone:?}"
        );
        // a folder of the person's own is the one looked in
        let elsewhere = scratch.join("elsewhere");
        std::fs::create_dir_all(&elsewhere).unwrap();
        std::fs::write(elsewhere.join("c-Q4_K_M.gguf"), b"GGUF").unwrap();
        let there = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &Wanted {
                models_dir: Some(elsewhere.clone()),
                ..wanted(Some("c-Q4_K_M.gguf"))
            },
            &mut notes,
        )
        .expect("it is in the folder that was named");
        assert_eq!(
            spawn_of(&there).1[1],
            elsewhere.join("c-Q4_K_M.gguf").display().to_string()
        );
    }

    /// O71: the values asked for are the plan's, and the program is the one named, wherever the document
    /// would have looked.
    #[test]
    fn p3_o71_the_values_and_the_program_asked_for_are_the_plans() {
        let (scratch, dirs) = world("o71");
        let registry = build_registry(&ConfigDirs::default());
        std::fs::create_dir_all(scratch.join("models")).unwrap();
        std::fs::write(scratch.join("models/m-Q4_K_M.gguf"), b"GGUF").unwrap();
        let mut wanted = Wanted {
            engine: "llamacpp".into(),
            binary: Some("/opt/x/llama-server".into()),
            ..Wanted::default()
        };
        wanted
            .values
            .insert("llamacpp.port".into(), Value::from(9123u16));
        wanted
            .values
            .insert("llamacpp.ctx_size".into(), Value::from(2048u32));
        let resolved = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &wanted,
            &mut Vec::new(),
        )
        .expect("it resolves");
        assert_eq!(resolved.binary, PathBuf::from("/opt/x/llama-server"));
        let (program, args) = spawn_of(&resolved);
        assert_eq!(program, "/opt/x/llama-server");
        let after = |flag: &str| args[args.iter().position(|a| a == flag).unwrap() + 1].clone();
        assert_eq!(after("--port"), "9123");
        assert_eq!(after("--ctx-size"), "2048");
        assert_eq!(resolved.plan.public_port, Some(9123));
    }

    /// O72: an engine that runs on its own is given no model, however one is named; an engine that takes
    /// models by name is given the name as it was said; an engine nobody has is an answer.
    #[test]
    fn p3_o72_an_engine_that_runs_alone_gets_no_model_and_one_that_takes_names_gets_the_name() {
        let (_scratch, dirs) = world("o72");
        let registry = build_registry(&ConfigDirs::default());
        let alone = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &Wanted {
                engine: "comfyui".into(),
                model_name: Some("whatever.gguf".into()),
                ..Wanted::default()
            },
            &mut Vec::new(),
        )
        .expect("ComfyUI resolves with no model");
        assert!(alone.model.is_none());

        let named = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &Wanted {
                engine: "ollama".into(),
                model_name: Some("qwen3:8b".into()),
                ..Wanted::default()
            },
            &mut Vec::new(),
        )
        .expect("Ollama resolves with a name");
        assert_eq!(named.model.as_ref().unwrap().id, "qwen3:8b");

        let nobody = resolve(
            &registry,
            &dirs,
            MachineFacts::default(),
            &Wanted {
                engine: "nosuch".into(),
                ..Wanted::default()
            },
            &mut Vec::new(),
        );
        assert!(matches!(nobody, Err(ResolveError::UnknownEngine(id)) if id == "nosuch"));
    }
}
