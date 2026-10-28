//! The configurations of the person: the files in their configuration directory, the working state
//! they describe, and what choosing, saving and sharing do (docs/configurations.md, sections 3, 6
//! and 7). The core says what a configuration is and what a person should be told about one; this
//! module is the effectful part: it reads and writes the files, reads the computer, and carries a
//! configuration to and from `settings.json`.
//!
//! The working state stays `settings.json`: nothing here changes what it is, and the one key it adds
//! (`values."app.configuration"`, the id of the active configuration) is absent while that is the
//! Default, so the file of a person who never uses configurations is what it always was.

// The windows of work packages 4.17.5 and 4.17.6 are the rest of its users.
#![allow(dead_code)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};

use quickai_core::config::Layer;
use quickai_core::doc::engine::FieldType;
use quickai_core::profile::check::{
    accepted, export, open_text, review, BuildHere, EngineHere, ExportMeta, FileHere, Inventory,
    ModelsHere, NamedHere, Opened, Report,
};
use quickai_core::profile::notes::Note;
use quickai_core::profile::state::{
    apply, capture, Applied, Captured, ModelFacts, WorkingState, MODEL_FIELD,
};
use quickai_core::profile::{
    clean_text, family, requires_for, Configuration, MadeOn, ModelSource, Runtime, MAX_NAME_CHARS,
};
use quickai_core::registry::{Engine, Registry};
use quickai_core::value::Value;

use crate::config::{build_registry, ConfigDirs};
use crate::gpu::{self, GpuVendor};
use crate::launch::MachineFacts;
use crate::prims::install::{list_installed, InstallSpec, InstalledBuild};
use crate::settings::Settings;

/// The folder of the person's configuration directory the files are in.
pub const PROFILES_DIR: &str = "profiles";
/// The id of the configuration that is always there.
pub const DEFAULT_ID: &str = "default";

/// The builds of each engine that are installed, by the id of the engine.
pub type Installed = BTreeMap<String, Vec<InstalledBuild>>;

// ---- what there is ---------------------------------------------------------------------------------

/// Where a configuration comes from, as the dropdown says it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Origin {
    /// Inside the application.
    BuiltIn,
    /// Shipped by the package, in `<exe>/config/profiles/`.
    Package,
    /// A file of the person's own.
    Yours,
}

impl Origin {
    pub fn of(layer: Layer) -> Origin {
        match layer {
            Layer::Embedded => Origin::BuiltIn,
            Layer::Bundle | Layer::Remote => Origin::Package,
            Layer::User | Layer::Override => Origin::Yours,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Origin::BuiltIn => "built in",
            Origin::Package => "from this package",
            Origin::Yours => "yours",
        }
    }
}

/// One configuration the registry has.
#[derive(Clone, Debug)]
pub struct Entry {
    pub id: String,
    pub name: String,
    pub origin: Origin,
    /// The file that is in force, as diagnostics name it.
    pub file: String,
    pub configuration: Configuration,
    /// What is odd about the file itself.
    pub notes: Vec<Note>,
}

/// The configurations of the registry: the Default first, the rest by name.
pub fn entries(registry: &Registry) -> Vec<Entry> {
    let mut all: Vec<Entry> = registry
        .profiles()
        .map(|p| Entry {
            id: p.configuration.id.clone(),
            name: p.configuration.name.clone(),
            origin: Origin::of(p.layer),
            file: p.file.clone(),
            configuration: p.configuration.clone(),
            notes: p.notes.clone(),
        })
        .collect();
    all.sort_by_key(|e| (e.id != DEFAULT_ID, e.name.to_lowercase(), e.id.clone()));
    all
}

/// A file in the person's folder that could not be used, and why.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Unusable {
    pub file: String,
    pub why: String,
}

/// The files of a `profiles` folder that the registry left out, with the sentence that says why.
pub fn unusable(registry: &Registry) -> Vec<Unusable> {
    registry
        .diagnostics
        .iter()
        .filter(|d| d.is_error())
        .filter_map(|d| {
            let file = d.file.as_ref()?;
            let path = Path::new(file);
            let in_profiles = path
                .parent()
                .and_then(Path::file_name)
                .is_some_and(|n| n == PROFILES_DIR);
            in_profiles.then(|| Unusable {
                file: file.clone(),
                why: match &d.hint {
                    Some(hint) => format!("{}: {hint}", d.message),
                    None => d.message.clone(),
                },
            })
        })
        .collect()
}

// ---- what can go wrong with the files ----------------------------------------------------------------

/// Why a configuration could not be saved, renamed, deleted or imported.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StoreError {
    /// There is no configuration directory to keep files in.
    NoFolder,
    /// QuickAI was started in safe mode: the person's own configurations are neither used nor changed.
    SafeMode,
    /// The name cannot be used.
    BadName(String),
    /// A configuration with that name is there.
    Taken(String),
    /// There is no configuration with that id.
    Missing(String),
    /// It is not the person's to change: it is built in, or the package's.
    NotYours(String),
    /// The operating system said no.
    Io { what: String, why: String },
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StoreError::NoFolder => write!(
                f,
                "There is no folder for configurations on this computer (the configuration directory could not be found)."
            ),
            StoreError::SafeMode => write!(
                f,
                "QuickAI was started in safe mode, so your own configurations are neither used nor changed."
            ),
            StoreError::BadName(why) => write!(f, "{why}"),
            StoreError::Taken(name) => write!(f, "There is already a configuration called {name}."),
            StoreError::Missing(id) => write!(f, "There is no configuration called {id}."),
            StoreError::NotYours(name) => write!(
                f,
                "{name} is built into QuickAI or comes with its package, so it cannot be removed or renamed; save it under another name to change it."
            ),
            StoreError::Io { what, why } => write!(f, "{what}: {why}"),
        }
    }
}

impl std::error::Error for StoreError {}

fn io(what: impl Into<String>, error: std::io::Error) -> StoreError {
    StoreError::Io {
        what: what.into(),
        why: error.to_string(),
    }
}

// ---- names and ids -------------------------------------------------------------------------------------

/// A name as the list shows it: cleaned, not empty, not longer than 60 characters.
pub fn check_name(name: &str) -> Result<String, StoreError> {
    let cleaned = clean_text(name, MAX_NAME_CHARS + 1, false);
    if cleaned.is_empty() {
        return Err(StoreError::BadName("A configuration needs a name.".into()));
    }
    if cleaned.chars().count() > MAX_NAME_CHARS {
        return Err(StoreError::BadName(format!(
            "A name has at most {MAX_NAME_CHARS} characters."
        )));
    }
    Ok(cleaned)
}

/// The id a name makes: lower case letters, digits and `-`, no more than 40 of them; never a path.
pub fn slug(name: &str) -> String {
    let mut out = String::new();
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed: String = out.trim_end_matches('-').chars().take(40).collect();
    let trimmed = trimmed.trim_end_matches('-').to_string();
    if trimmed.is_empty() {
        "configuration".to_string()
    } else {
        trimmed
    }
}

/// An id for `name` that nothing has: the name's slug, and a number after it when that is taken. The
/// Default's id is never given to another configuration.
pub fn free_id(name: &str, taken: impl Fn(&str) -> bool) -> String {
    let base = slug(name);
    let unavailable = |id: &str| id == DEFAULT_ID || taken(id);
    if !unavailable(&base) {
        return base;
    }
    (2..)
        .map(|n| format!("{base}-{n}"))
        .find(|id| !unavailable(id))
        .expect("an unbounded count finds a free id")
}

/// Whether the registry has a configuration with this name (in any case), other than `except`.
fn name_in_use(registry: &Registry, name: &str, except: Option<&str>) -> bool {
    registry.profiles().any(|p| {
        Some(p.configuration.id.as_str()) != except
            && p.configuration.name.to_lowercase() == name.to_lowercase()
    })
}

// ---- the files -------------------------------------------------------------------------------------------

/// Where the person's configurations are written: below the override directory when there is one (the
/// tests', which is the layer that wins), else below the configuration directory.
pub fn write_dir(dirs: &ConfigDirs) -> Option<PathBuf> {
    if dirs.safe_mode {
        return None;
    }
    dirs.override_dir
        .as_ref()
        .or(dirs.user.as_ref())
        .map(|root| root.join(PROFILES_DIR))
}

/// Where the files go, or why they cannot.
fn folder(dirs: &ConfigDirs) -> Result<PathBuf, StoreError> {
    write_dir(dirs).ok_or(if dirs.safe_mode {
        StoreError::SafeMode
    } else {
        StoreError::NoFolder
    })
}

fn id_taken(dirs: &ConfigDirs, registry: &Registry, id: &str) -> bool {
    registry.profile(id).is_some()
        || write_dir(dirs).is_some_and(|dir| dir.join(format!("{id}.toml")).exists())
}

/// Whether a document of the same id is in a layer below the person's: a file written for it replaces
/// it, and says so (`replace = true`), so that the two are never merged.
fn lower_layers_have(dirs: &ConfigDirs, id: &str) -> bool {
    let lower = ConfigDirs {
        user: None,
        override_dir: None,
        ..dirs.clone()
    };
    build_registry(&lower).profile(id).is_some()
}

/// Writes `text` to `path` whole or not at all: to a file beside it first, which is then renamed.
fn write_atomically(path: &Path, text: &str) -> Result<(), StoreError> {
    let dir = path.parent().ok_or(StoreError::NoFolder)?;
    std::fs::create_dir_all(dir).map_err(|e| io(format!("Could not make {}", dir.display()), e))?;
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let partial = dir.join(format!(".{name}.{}.partial", std::process::id()));
    std::fs::write(&partial, text)
        .map_err(|e| io(format!("Could not write {}", partial.display()), e))?;
    std::fs::rename(&partial, path).map_err(|e| {
        let _ = std::fs::remove_file(&partial);
        io(format!("Could not write {}", path.display()), e)
    })
}

/// Writes `configuration` as the person's own file: the one that already holds its id when there is
/// one, else `<id>.toml` in their folder. Returns the file.
pub fn write(
    dirs: &ConfigDirs,
    registry: &Registry,
    configuration: &Configuration,
) -> Result<PathBuf, StoreError> {
    let dir = folder(dirs)?;
    let existing = registry
        .profile(&configuration.id)
        .filter(|p| Origin::of(p.layer) == Origin::Yours);
    let path = match existing {
        Some(entry) => PathBuf::from(&entry.file),
        None => dir.join(format!("{}.toml", configuration.id)),
    };
    let replace = lower_layers_have(dirs, &configuration.id)
        || existing.is_some_and(|entry| file_replaces(&entry.file));
    let text = configuration.to_toml(registry.engine_map(), env!("CARGO_PKG_VERSION"), replace);
    write_atomically(&path, &text)?;
    Ok(path)
}

/// Whether the file at `path` says `replace = true`.
fn file_replaces(path: &str) -> bool {
    std::fs::read_to_string(path).is_ok_and(|text| quickai_core::profile::replaces(&text))
}

/// What a saved configuration says about where it was made: this version and the kind of computer.
pub fn stamp(configuration: &mut Configuration, host: &Host) {
    configuration.meta.made_with = Some(format!("QuickAI {}", env!("CARGO_PKG_VERSION")));
    configuration.meta.made_on = Some(host.made_on());
}

/// Saves the working state over the configuration `id`, which must be the person's own or one that
/// they now make their own (the Default, a package's): the whole file, from the state, or nothing.
pub fn save(
    dirs: &ConfigDirs,
    registry: &Registry,
    id: &str,
    captured: Captured,
    host: &Host,
) -> Result<PathBuf, StoreError> {
    let existing = registry
        .profile(id)
        .ok_or_else(|| StoreError::Missing(id.to_string()))?;
    let name = existing.configuration.name.clone();
    let kept = &existing.configuration;
    // the models of the file are kept while the window shows the first of them: it holds one, and a file may
    // say several, each with where it can be had from (the files of a video model)
    let models = if captured
        .configuration
        .models
        .first()
        .map(|m| m.source.label())
        == kept.models.first().map(|m| m.source.label())
    {
        kept.models.clone()
    } else {
        captured.configuration.models.clone()
    };
    let mut configuration = Captured {
        configuration: Configuration {
            // what the file holds that this version does not know is kept
            extra: kept.extra.clone(),
            meta: kept.meta.clone(),
            // and what the window has no place for: what the file needs from whoever runs it, and the build
            // of each program that goes with the engine
            needs: kept.needs.clone(),
            runtime: Runtime {
                companions: kept.runtime.companions.clone(),
                ..captured.configuration.runtime.clone()
            },
            models,
            ..captured.configuration
        },
        ..captured
    }
    .named(id, &name)
    .configuration;
    stamp(&mut configuration, host);
    write(dirs, registry, &configuration)
}

/// Saves the working state as a new configuration with this name. Returns its id.
pub fn save_as(
    dirs: &ConfigDirs,
    registry: &Registry,
    name: &str,
    captured: Captured,
    host: &Host,
) -> Result<String, StoreError> {
    let name = check_name(name)?;
    if name_in_use(registry, &name, None) {
        return Err(StoreError::Taken(name));
    }
    let id = free_id(&name, |id| id_taken(dirs, registry, id));
    let mut configuration = captured.named(&id, &name).configuration;
    stamp(&mut configuration, host);
    write(dirs, registry, &configuration)?;
    Ok(id)
}

/// Changes the name of a configuration of the person's, and nothing else of it.
pub fn rename(
    dirs: &ConfigDirs,
    registry: &Registry,
    id: &str,
    name: &str,
) -> Result<(), StoreError> {
    let entry = registry
        .profile(id)
        .ok_or_else(|| StoreError::Missing(id.to_string()))?;
    let name = check_name(name)?;
    if name_in_use(registry, &name, Some(id)) {
        return Err(StoreError::Taken(name));
    }
    let mut configuration = entry.configuration.clone();
    configuration.name = name;
    write(dirs, registry, &configuration)?;
    Ok(())
}

/// A copy of a configuration under a name of its own. Returns the id.
pub fn duplicate(
    dirs: &ConfigDirs,
    registry: &Registry,
    id: &str,
    name: &str,
) -> Result<String, StoreError> {
    let entry = registry
        .profile(id)
        .ok_or_else(|| StoreError::Missing(id.to_string()))?;
    let name = check_name(name)?;
    if name_in_use(registry, &name, None) {
        return Err(StoreError::Taken(name));
    }
    let new_id = free_id(&name, |id| id_taken(dirs, registry, id));
    let mut configuration = entry.configuration.clone();
    configuration.id = new_id.clone();
    configuration.name = name;
    write(dirs, registry, &configuration)?;
    Ok(new_id)
}

/// Removes the person's own file for a configuration. What was below it comes back: the built-in
/// Default for the person's Default. A configuration that has no file of the person's is not theirs to
/// remove.
pub fn delete(dirs: &ConfigDirs, registry: &Registry, id: &str) -> Result<(), StoreError> {
    let entry = registry
        .profile(id)
        .ok_or_else(|| StoreError::Missing(id.to_string()))?;
    if Origin::of(entry.layer) != Origin::Yours {
        return Err(StoreError::NotYours(entry.configuration.name.clone()));
    }
    if id == DEFAULT_ID && !lower_layers_have(dirs, id) {
        return Err(StoreError::NotYours(entry.configuration.name.clone()));
    }
    std::fs::remove_file(&entry.file).map_err(|e| io(format!("Could not remove {}", entry.file), e))
}

/// The person's own Default is removed, and the built-in one is the Default again.
pub fn reset_to_factory(dirs: &ConfigDirs, registry: &Registry) -> Result<(), StoreError> {
    match registry.profile(DEFAULT_ID) {
        Some(entry) if Origin::of(entry.layer) == Origin::Yours => {
            delete(dirs, registry, DEFAULT_ID)
        }
        _ => Ok(()),
    }
}

// ---- the computer ----------------------------------------------------------------------------------------

/// What is known of the computer, for the notes and for what a saved file says about where it was made.
#[derive(Clone, Debug, Default)]
pub struct Host {
    pub os: String,
    pub arch: String,
    pub ram_bytes: u64,
    /// Lower-case makers (`amd`, `nvidia`).
    pub gpu_vendors: Vec<String>,
}

impl Host {
    pub fn from_machine(machine: &MachineFacts) -> Host {
        Host {
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            ram_bytes: machine.ram_bytes,
            gpu_vendors: machine.gpu_vendors.clone(),
        }
    }

    pub fn made_on(&self) -> MadeOn {
        MadeOn {
            os: Some(self.os.clone()).filter(|o| !o.is_empty()),
            arch: Some(self.arch.clone()).filter(|a| !a.is_empty()),
            gpu: self.gpu_vendors.clone(),
            memory_gb: (self.ram_bytes > 0)
                .then(|| (self.ram_bytes as f64 / (1024.0 * 1024.0 * 1024.0)).round() as u64),
        }
    }

    fn vendors(&self) -> Vec<GpuVendor> {
        self.gpu_vendors
            .iter()
            .filter_map(|v| GpuVendor::from_key(v))
            .collect()
    }
}

/// The builds installed for each engine of the registry, as the manager lists them.
pub fn installed_builds(registry: &Registry, facts: &quickai_core::plan::HostFacts) -> Installed {
    registry
        .engines()
        .filter_map(|engine| {
            let spec = InstallSpec::from_engine(engine, facts)?;
            Some((engine.id.clone(), list_installed(&spec)))
        })
        .collect()
}

/// What each engine that keeps its own models listed the last time it was running, by the id of the
/// engine: the remembered list, which is all that can be said while it is stopped. An engine that
/// never listed has no entry.
pub fn listed_by_engines(
    registry: &Registry,
    facts: &quickai_core::plan::HostFacts,
) -> BTreeMap<String, Vec<NamedHere>> {
    use crate::prims::engine_models::{read_cache, ListSpec};
    let mut out = BTreeMap::new();
    for engine in registry.engines() {
        let Some(Ok(call)) = quickai_core::models::list_call(engine, facts) else {
            continue;
        };
        let spec = ListSpec::from_call(&call);
        let Some(rows) = spec.cache.as_deref().and_then(read_cache) else {
            continue;
        };
        out.insert(
            engine.id.clone(),
            rows.into_iter()
                .filter(|row| !spec.hide.iter().any(|pattern| pattern.is_match(&row.id)))
                .map(|row| NamedHere {
                    name: row.id,
                    size: row.size_bytes,
                })
                .collect(),
        );
    }
    out
}

/// The models folders of the application: the ones that come with it, and the ones it downloads to.
pub fn models_here(
    registry: &Registry,
    bundled_dir: &Path,
    user_dir_of: &dyn Fn(&Engine) -> PathBuf,
    listed: &BTreeMap<String, Vec<NamedHere>>,
) -> BTreeMap<String, ModelsHere> {
    let mut out = BTreeMap::new();
    for engine in registry.engines() {
        let value = if engine
            .doc
            .provides
            .accepts
            .as_ref()
            .is_some_and(|a| !a.refs.is_empty())
        {
            ModelsHere::Named {
                listed: listed.get(&engine.id).cloned(),
            }
        } else if engine.doc.provides.needs_model {
            let hide = quickai_core::lookup::bundle::companion_patterns(
                quickai_core::models::bundles(engine),
            );
            ModelsHere::Files(
                crate::models::discover_all(bundled_dir, &user_dir_of(engine), &hide)
                    .into_iter()
                    .map(|m| FileHere {
                        name: m.filename,
                        size: m.size_bytes,
                    })
                    .collect(),
            )
        } else {
            ModelsHere::Unknown
        };
        out.insert(engine.id.clone(), value);
    }
    out
}

/// What is going on on this computer at this moment, for a review: which engines run, which of the ports a
/// configuration sets are taken, and the address of an engine that each program that goes with one was first
/// started with.
pub struct Now<'a> {
    pub running: &'a BTreeSet<String>,
    pub busy_ports: BTreeSet<u16>,
    pub first_addresses: BTreeMap<String, String>,
}

/// Everything a review needs to know about this computer.
pub fn inventory(
    registry: &Registry,
    host: &Host,
    installed: &Installed,
    models: &BTreeMap<String, ModelsHere>,
    now: Now<'_>,
    program_found: &dyn Fn(&Engine) -> bool,
) -> Inventory {
    let Now {
        running,
        busy_ports,
        first_addresses,
    } = now;
    let vendors = host.vendors();
    let mut engines = BTreeMap::new();
    for engine in registry.engines() {
        let builds: Vec<BuildHere> = installed
            .get(&engine.id)
            .map(|list| {
                list.iter()
                    .map(|b| BuildHere {
                        label: b.label.clone(),
                        warning: quickai_core::acquire::backend(engine, &b.label).and_then(
                            |backend| gpu::mismatch_warning(&vendors, backend, &backend.label),
                        ),
                    })
                    .collect()
            })
            .unwrap_or_default();
        engines.insert(
            engine.id.clone(),
            EngineHere {
                installed: !builds.is_empty() || program_found(engine),
                running: running.contains(&engine.id),
                builds,
                models: models.get(&engine.id).cloned().unwrap_or_default(),
            },
        );
    }
    Inventory {
        app_version: env!("CARGO_PKG_VERSION").to_string(),
        os: host.os.clone(),
        arch: host.arch.clone(),
        ram_bytes: host.ram_bytes,
        engines,
        busy_ports,
        companion_first: first_addresses,
    }
}

/// The address of the engine each program that goes with one was first started with (a value of its own in
/// the settings, in the field its document names), by the id of the program.
pub fn first_addresses(registry: &Registry, settings: &Settings) -> BTreeMap<String, String> {
    registry
        .engines()
        .filter(|e| e.doc.engine.companion_of.is_some())
        .filter_map(|engine| {
            let field = engine.doc.engine.first_address_field.as_deref()?;
            let key = quickai_core::profile::value_key(&engine.id, field);
            let value = settings
                .engine_values(engine)
                .get(&key)?
                .as_str()?
                .to_string();
            (!value.is_empty()).then(|| (engine.id.clone(), value))
        })
        .collect()
}

/// The ports in use on this computer now among those a configuration sets, apart from the ones the
/// working state already has (those are ours).
pub fn busy_ports(
    registry: &Registry,
    configuration: &Configuration,
    state: &WorkingState,
) -> BTreeSet<u16> {
    let mut busy = BTreeSet::new();
    for member in family(registry.engine_map(), &configuration.engine) {
        for field in member.doc.fields.iter().filter(|f| f.ty == FieldType::Port) {
            let key = quickai_core::profile::value_key(&member.id, &field.id);
            let Some(port) = configuration
                .values
                .get(&key)
                .and_then(Value::as_i64)
                .and_then(|p| u16::try_from(p).ok())
            else {
                continue;
            };
            if state.values.get(&key).and_then(Value::as_i64) == Some(i64::from(port)) {
                continue;
            }
            if std::net::TcpListener::bind(("127.0.0.1", port)).is_err() {
                busy.insert(port);
            }
        }
    }
    busy
}

// ---- the working state and settings.json ---------------------------------------------------------------

/// The id of the field that holds the engine's program, when it has one.
pub fn runtime_field(engine: &Engine) -> Option<&str> {
    engine
        .doc
        .fields
        .iter()
        .find(|f| f.ty == FieldType::RuntimePath)
        .map(|f| f.id.as_str())
}

/// What each engine's first run stores, for the settings that have always been in the settings file:
/// the values that count as the default of a configuration.
pub fn first_run_values(registry: &Registry) -> BTreeMap<String, Value> {
    let fresh = Settings::default();
    registry
        .engines()
        .flat_map(|engine| fresh.engine_values(engine))
        .collect()
}

/// The working state of `settings`: the engine in use, the settings of it and of what goes with it, the
/// build when the program is an installed one, and the proxy.
pub fn working_state(
    registry: &Registry,
    settings: &Settings,
    installed: &Installed,
) -> WorkingState {
    let (engine_id, _) = crate::config::engine_in_use(registry, settings);
    let members = family(registry.engine_map(), &engine_id);
    let mut values = BTreeMap::new();
    for member in &members {
        values.extend(settings.engine_values(member));
    }
    let build = members.first().and_then(|head| {
        let field = runtime_field(head)?;
        let path = values
            .get(&quickai_core::profile::value_key(&head.id, field))?
            .as_str()?;
        installed
            .get(&head.id)?
            .iter()
            .find(|b| Path::new(path) == b.binary_path)
            .map(|b| b.label.clone())
    });
    WorkingState {
        engine: engine_id,
        values,
        build,
        traffic_proxy: settings.enable_traffic_proxy,
    }
}

/// Carries `state` into `settings`: the engine, the settings of the engine and what goes with it, and
/// the proxy. What a configuration does not carry (the theme, the consents, other engines' values)
/// stays as it is.
pub fn store_state(
    registry: &Registry,
    settings: &mut Settings,
    state: &WorkingState,
) -> Result<(), String> {
    let mut next = settings.clone();
    for member in family(registry.engine_map(), &state.engine) {
        let own: BTreeMap<String, Value> = state
            .values
            .iter()
            .filter(|(key, _)| {
                quickai_core::profile::split_key(key).is_some_and(|(owner, _)| owner == member.id)
            })
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        next.set_engine_values(member, &own)?;
    }
    next.set_chosen_engine(&state.engine);
    next.enable_traffic_proxy = state.traffic_proxy;
    *settings = next;
    Ok(())
}

/// The model of the engine that the working state has, with what is known of it.
pub fn model_facts(
    state: &WorkingState,
    models: &BTreeMap<String, ModelsHere>,
) -> Option<ModelFacts> {
    let name = state
        .values
        .get(&quickai_core::profile::value_key(
            &state.engine,
            MODEL_FIELD,
        ))?
        .as_str()?
        .to_string();
    match models.get(&state.engine)? {
        ModelsHere::Files(files) => files.iter().find(|f| f.name == name).map(|f| ModelFacts {
            size: Some(f.size),
            ..ModelFacts::default()
        }),
        ModelsHere::Named { listed: Some(list) } => {
            list.iter().find(|m| m.name == name).map(|m| ModelFacts {
                size: m.size,
                ..ModelFacts::default()
            })
        }
        _ => None,
    }
}

// ---- the active configuration -----------------------------------------------------------------------------

/// The configuration to start with: the one the settings name, else the Default. A name the registry
/// does not know (a deleted file, a safe-mode start) is the Default too, with a sentence for the log;
/// the working state is not touched either way.
pub fn active_for_start(registry: &Registry, settings: &Settings) -> (String, Option<String>) {
    match settings.active_configuration() {
        Some(id) if registry.profile(&id).is_some() => (id, None),
        Some(id) => (
            DEFAULT_ID.to_string(),
            Some(format!(
                "config: the settings name the configuration `{id}`, which is not here; the Default is the one in use. The settings are as they were."
            )),
        ),
        None => (DEFAULT_ID.to_string(), None),
    }
}

/// The first start with configurations adopts what the person had set: when it is not what the built-in
/// Default describes, it is written once as the person's Default, so that nobody who tuned their
/// settings sees unsaved changes on the first morning. The folder `profiles/` is what remembers that it
/// was done: it is made here, and where it is already, nothing is written.
///
/// `settings` are the settings as the file had them, not what the window made of them: a model that
/// the application picked for a person who never chose one is not something they had set, and is not
/// in the Default. When the settings do name a model, `model` is what is known of that file, so that
/// the Default names it the way the window will compare it (by the file it is, not by the list
/// that happens to have an entry of that name).
pub fn adopt(
    dirs: &ConfigDirs,
    registry: &Registry,
    settings: &Settings,
    installed: &Installed,
    host: &Host,
    model: Option<&ModelFacts>,
) -> Result<Option<PathBuf>, StoreError> {
    if dirs.safe_mode {
        return Ok(None);
    }
    let dir = folder(dirs)?;
    if dir.exists() {
        return Ok(None);
    }
    std::fs::create_dir_all(&dir)
        .map_err(|e| io(format!("Could not make {}", dir.display()), e))?;
    let Some(built_in) = registry.profile(DEFAULT_ID) else {
        return Ok(None);
    };
    let state = working_state(registry, settings, installed);
    let captured = capture(registry, &state, &first_run_values(registry), model);
    if captured.configuration.matches_run(&built_in.configuration) {
        return Ok(None);
    }
    let mut configuration = captured
        .named(DEFAULT_ID, &built_in.configuration.name)
        .configuration;
    stamp(&mut configuration, host);
    Ok(Some(write(dirs, registry, &configuration)?))
}

/// A configuration that is the active one is gone: the Default is the active one. Returns whether
/// `settings` changed.
pub fn forget_if_active(settings: &mut Settings, id: &str) -> bool {
    if settings.active_configuration().as_deref() == Some(id) {
        settings.set_active_configuration(DEFAULT_ID);
        true
    } else {
        false
    }
}

// ---- choosing --------------------------------------------------------------------------------------------

/// What choosing a configuration makes of the working state, before anything is stored.
#[derive(Clone, Debug)]
pub struct Choice {
    pub state: WorkingState,
    pub applied: Applied,
    /// What the person is told, most important first.
    pub report: Report,
}

/// The model name the engine is given for the configuration's model, when the model is here (or cannot
/// be told: an engine that is stopped does not say what it has).
fn model_to_select(configuration: &Configuration, here: &Inventory) -> Option<String> {
    let pick = configuration.models.first()?;
    let on_hand = here.engines.get(&configuration.engine)?;
    match (&pick.source, &on_hand.models) {
        (ModelSource::Named { name, .. }, ModelsHere::Named { listed }) => match listed {
            Some(list) if list.iter().any(|m| &m.name == name) => Some(name.clone()),
            Some(_) if on_hand.running => None,
            _ => Some(name.clone()),
        },
        (
            ModelSource::File { name }
            | ModelSource::Catalog {
                file: Some(name), ..
            },
            ModelsHere::Files(files),
        ) => files
            .iter()
            .find(|f| &f.name == name && pick.size.is_none_or(|s| s == 0 || f.size == s))
            .map(|f| f.name.clone()),
        (source, ModelsHere::Unknown) => Some(source.label().to_string()),
        _ => None,
    }
}

/// Chooses `configuration` for the working state `settings` holds: the notes for this computer are
/// made, and when none stops it the state it makes is returned. `Err` is the report that stops it.
/// Nothing is stored, started, fetched or installed here.
pub fn choose(
    registry: &Registry,
    settings: &Settings,
    configuration: &Configuration,
    here: &Inventory,
    installed: &Installed,
) -> Result<Choice, Report> {
    let mut report = review(registry, configuration, &[], here, false);
    if report.stop().is_some() {
        return Err(report);
    }
    let applied = apply(registry, configuration).map_err(|stop| Report { notes: vec![stop] })?;
    // what applying itself left out is told too, once
    for note in &applied.notes {
        if !report
            .notes
            .iter()
            .any(|n| n.id == note.id && n.subject == note.subject && n.holes == note.holes)
        {
            report.notes.push(note.clone());
        }
    }
    quickai_core::profile::notes::sort(&mut report.notes);
    let mut base = working_state(registry, settings, installed);
    // A configuration of another engine starts from that engine's values as the file holds them: what it does not
    // carry (the folders, the program, the lines of environment) rides along. From the engine in use alone, storing
    // the state cleared every field of the other engine that the configuration did not set.
    if configuration.engine != base.engine {
        for member in family(registry.engine_map(), &configuration.engine) {
            for (key, value) in settings.engine_values(member) {
                base.values.entry(key).or_insert(value);
            }
        }
    }
    let build = configuration.runtime.build.as_deref().and_then(|label| {
        installed
            .get(&configuration.engine)?
            .iter()
            .find(|b| b.label == label)
    });
    let model = model_to_select(configuration, here);
    let mut state = base.after(
        registry,
        &applied,
        model.as_deref(),
        build.map(|b| b.label.as_str()),
    );
    // the program is the installed build of that label, and never a path a file gave
    if let (Some(build), Some(head)) = (build, registry.engine(&configuration.engine)) {
        if let Some(field) = runtime_field(head) {
            state.values.insert(
                quickai_core::profile::value_key(&head.id, field),
                Value::Str(build.binary_path.display().to_string()),
            );
        }
    }
    Ok(Choice {
        state,
        applied,
        report,
    })
}

// ---- sharing ---------------------------------------------------------------------------------------------

/// How to treat a file whose id is already used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OnTaken {
    /// Refuse: the person is asked.
    Refuse,
    /// Keep both: the new one takes another id and name.
    KeepBoth,
    /// The new one stands in for the old (a file of the person's own).
    Replace,
}

/// Whether importing `configuration` (under `called`, when the person gave it a name) would meet a
/// configuration that has its id or its name: the person is asked what to do.
pub fn import_is_taken(
    dirs: &ConfigDirs,
    registry: &Registry,
    configuration: &Configuration,
    called: Option<&str>,
) -> bool {
    let name = match called {
        Some(name) => check_name(name).unwrap_or_else(|_| configuration.name.clone()),
        None => configuration.name.clone(),
    };
    let preferred = match called {
        Some(_) => slug(&name),
        None => configuration.id.clone(),
    };
    preferred == DEFAULT_ID
        || id_taken(dirs, registry, &preferred)
        || name_in_use(registry, &name, None)
}

/// What importing a file did or would do.
#[derive(Clone, Debug)]
pub struct Imported {
    pub configuration: Configuration,
    pub report: Report,
    /// The file written; `None` for a check.
    pub file: Option<PathBuf>,
}

/// Imports the text of a file from outside: it is opened, looked at for this computer, and what the
/// person ticked (`ticked`: the keys of the settings asked about) is stored as one of their own
/// configurations. Reading and looking use no network and start nothing; the one file written is the
/// configuration, in the person's folder.
#[allow(clippy::too_many_arguments)]
pub fn import(
    dirs: &ConfigDirs,
    registry: &Registry,
    text: &str,
    here: &Inventory,
    ticked: &BTreeSet<String>,
    called: Option<&str>,
    on_taken: OnTaken,
    write_it: bool,
) -> Result<Imported, Report> {
    let Opened {
        configuration,
        notes,
    } = open_text(registry, text, env!("CARGO_PKG_VERSION"))
        .map_err(|stop| Report { notes: vec![stop] })?;
    let mut report = review(registry, &configuration, &notes, here, true);
    if report.stop().is_some() {
        return Err(report);
    }
    let (mut stored, refused) = accepted(registry, &configuration, ticked);
    for note in refused {
        let duplicate = report
            .notes
            .iter()
            .any(|n| n.id == note.id && n.subject == note.subject && n.holes == note.holes);
        if !duplicate {
            report.notes.push(note);
        }
    }
    let name = match called {
        Some(name) => check_name(name),
        None => check_name(&stored.name),
    }
    .map_err(|e| Report {
        notes: vec![Note::new("not_a_configuration", &[("why", e.to_string())])],
    })?;
    // the file's own id, unless the person gave it a name, which makes the id
    let preferred = match called {
        Some(_) => slug(&name),
        None => stored.id.clone(),
    };
    let taken = preferred == DEFAULT_ID
        || id_taken(dirs, registry, &preferred)
        || name_in_use(registry, &name, None);
    let (id, name) = match (taken, on_taken) {
        (false, _) => (preferred, name),
        (true, OnTaken::Refuse) => {
            report
                .notes
                .push(Note::new("name_taken", &[("name", name.clone())]));
            return Err(report);
        }
        (true, OnTaken::KeepBoth) => {
            let mut candidate = name.clone();
            let mut n = 2;
            while name_in_use(registry, &candidate, None) {
                candidate = format!("{name} {n}");
                n += 1;
            }
            // the id follows the name it ends up with
            let id = free_id(&candidate, |id| id_taken(dirs, registry, id));
            (id, candidate)
        }
        (true, OnTaken::Replace) => (
            registry
                .profiles()
                .find(|p| {
                    p.configuration.id == preferred
                        || p.configuration.name.to_lowercase() == name.to_lowercase()
                })
                .map_or(preferred, |p| p.configuration.id.clone()),
            name,
        ),
    };
    stored.id = id;
    stored.name = name;
    let file = if write_it {
        Some(write(dirs, registry, &stored).map_err(|e| Report {
            notes: vec![Note::new("not_a_configuration", &[("why", e.to_string())])],
        })?)
    } else {
        None
    };
    quickai_core::profile::notes::sort(&mut report.notes);
    Ok(Imported {
        configuration: stored,
        report,
        file,
    })
}

/// The text of a file to give away from a stored configuration: the secrets taken out, the kind of
/// computer it was made on, and what the person said about it. Returns the text and what was left out.
pub fn export_text(
    registry: &Registry,
    id: &str,
    name: Option<&str>,
    note: Option<&str>,
    host: &Host,
) -> Result<(String, Vec<Note>), StoreError> {
    let entry = registry
        .profile(id)
        .ok_or_else(|| StoreError::Missing(id.to_string()))?;
    let captured = Captured {
        configuration: entry.configuration.clone(),
        left_out: Vec::new(),
    };
    let exported = export(
        registry,
        captured,
        &entry.configuration.id,
        name.unwrap_or(&entry.configuration.name),
        ExportMeta {
            note: note
                .map(str::to_string)
                .or_else(|| entry.configuration.meta.note.clone()),
            made_with: format!("QuickAI {}", env!("CARGO_PKG_VERSION")),
            made_on: host.made_on(),
        },
    );
    let mut configuration = exported.configuration;
    configuration.extra = entry.configuration.extra.clone();
    let text = configuration.to_toml(registry.engine_map(), env!("CARGO_PKG_VERSION"), false);
    Ok((text, exported.notes))
}

/// The text of a configuration made from what the window holds now (`captured`), to be run elsewhere: on another
/// computer, by QuickAI's own headless program (docs/remote-runs.md). It is written as `export_text` writes a stored
/// one, the secrets taken out; `id` and `name` are the ones of the configuration in use.
pub fn export_captured(
    registry: &Registry,
    captured: Captured,
    id: &str,
    name: &str,
    host: &Host,
) -> (String, Vec<Note>) {
    let exported = export(
        registry,
        captured,
        id,
        name,
        ExportMeta {
            note: None,
            made_with: format!("QuickAI {}", env!("CARGO_PKG_VERSION")),
            made_on: host.made_on(),
        },
    );
    let text =
        exported
            .configuration
            .to_toml(registry.engine_map(), env!("CARGO_PKG_VERSION"), false);
    (text, exported.notes)
}

/// The lowest application version that reads what this build writes, for the docs and the tests.
pub fn requires() -> String {
    requires_for(env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quickai-configurations-{name}-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn dirs_in(dir: &Path) -> ConfigDirs {
        ConfigDirs {
            user: Some(dir.to_path_buf()),
            data: Some(dir.join("data")),
            ..ConfigDirs::default()
        }
    }

    fn host() -> Host {
        Host {
            os: "linux".into(),
            arch: "x86_64".into(),
            ram_bytes: 32 * 1024 * 1024 * 1024,
            gpu_vendors: vec!["amd".into()],
        }
    }

    fn captured_now(registry: &Registry, settings: &Settings) -> Captured {
        let state = working_state(registry, settings, &Installed::new());
        capture(registry, &state, &first_run_values(registry), None)
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn p3_a41_the_store_writes_whole_makes_ids_from_names_and_never_uses_a_name_as_a_path() {
        // ids: lower case, letters, digits and `-`
        assert_eq!(slug("Vision chat"), "vision-chat");
        assert_eq!(slug("  Coding, small!  "), "coding-small");
        assert_eq!(slug("../../etc/passwd"), "etc-passwd");
        assert_eq!(slug("C:\\Users\\x"), "c-users-x");
        assert_eq!(slug("日本語"), "configuration");
        assert_eq!(slug(""), "configuration");
        assert!(slug(&"a b ".repeat(40)).len() <= 40);
        // a number is added when the id is taken, and the Default's is never given
        assert_eq!(free_id("Vision chat", |_| false), "vision-chat");
        let taken = ["vision-chat", "vision-chat-2"];
        assert_eq!(
            free_id("Vision chat", |id| taken.contains(&id)),
            "vision-chat-3"
        );
        assert_eq!(free_id("Default", |_| false), "default-2");
        // names: not empty, not too long
        assert!(check_name("   ").is_err());
        assert!(check_name(&"n".repeat(61)).is_err());
        assert_eq!(check_name("  A\u{202E}B  ").unwrap(), "AB");

        // a file is written whole, and nothing else is left in the folder
        let dir = scratch("a41");
        let dirs = dirs_in(&dir);
        let registry = build_registry(&dirs);
        let settings = Settings::default();
        let id = save_as(
            &dirs,
            &registry,
            "../../outside",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        assert_eq!(id, "outside");
        assert_eq!(files_in(&dir.join(PROFILES_DIR)), ["outside.toml"]);
        assert!(!dir.parent().unwrap().join("outside.toml").exists());
        assert!(!dir.join("outside.toml").exists());
        let text = std::fs::read_to_string(dir.join("profiles/outside.toml")).unwrap();
        assert!(
            text.contains("id = \"outside\"") && text.contains("name = \"../../outside\""),
            "{text}"
        );
        // and it is read back as it was written
        let again = build_registry(&dirs);
        assert!(
            again.diagnostics.iter().all(|d| !d.is_error()),
            "{:?}",
            again.diagnostics
        );
        assert_eq!(
            again.profile("outside").unwrap().configuration.name,
            "../../outside"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Y05: a Save from the window writes the whole file from what the window holds, and keeps what the window
    /// cannot hold: the files of a model that are not the one it shows, where each can be had from, what the file
    /// needs from whoever runs it, and the pin of a program that goes with the engine. A model chosen instead
    /// replaces the list; the needs and the pin stay.
    #[test]
    fn p3_y05_a_save_from_the_window_keeps_what_the_window_cannot_hold() {
        let dir = scratch("y05");
        let dirs = dirs_in(&dir);
        std::fs::create_dir_all(dir.join("profiles")).unwrap();
        let wan = r#"schema = 1
kind = "profile"
id = "wan"
name = "Wan"
engine = "llamacpp"

[[model]]
role = "diffusion"
file = "a.gguf"
from = "huggingface:acme/Repo"
path = "deep/a.gguf"
revision = "abc"
size = 10

[[model]]
role = "vae"
file = "b.gguf"
url = "https://mirror.example/b.gguf"

[[needs]]
token = "huggingface"
why = "A licence."
"#;
        std::fs::write(dir.join("profiles/wan.toml"), wan).unwrap();
        let mut registry = build_registry(&dirs);
        assert_eq!(
            registry.profile("wan").unwrap().configuration.models.len(),
            2
        );
        let mut settings = Settings {
            port: 8123,
            last_model_filename: Some("a.gguf".into()),
            ..Settings::default()
        };
        save(
            &dirs,
            &registry,
            "wan",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = build_registry(&dirs);
        let saved = &registry.profile("wan").unwrap().configuration;
        assert_eq!(
            saved.values["llamacpp.port"],
            Value::Int(8123),
            "what the window holds is written"
        );
        assert_eq!(saved.models.len(), 2, "{:?}", saved.models);
        assert_eq!(saved.models[0].path.as_deref(), Some("deep/a.gguf"));
        assert_eq!(saved.models[0].revision.as_deref(), Some("abc"));
        assert_eq!(
            saved.models[1].url.as_deref(),
            Some("https://mirror.example/b.gguf")
        );
        assert_eq!(saved.needs.len(), 1);
        assert_eq!(saved.needs[0].why.as_deref(), Some("A licence."));

        // another model chosen: the list is that one; what the file needs stays
        settings.last_model_filename = Some("c.gguf".into());
        save(
            &dirs,
            &registry,
            "wan",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = build_registry(&dirs);
        let saved = &registry.profile("wan").unwrap().configuration;
        assert_eq!(saved.models.len(), 1, "{:?}", saved.models);
        assert_eq!(saved.models[0].source.label(), "c.gguf");
        assert_eq!(saved.needs.len(), 1);

        // the pin of a program that goes with the engine
        let chat = r#"schema = 1
kind = "profile"
id = "chat"
name = "Chat"
engine = "ollama"

[runtime.openwebui]
build = "0.11.4"

[[needs]]
env = "WEBUI_SECRET_KEY"
for = "openwebui"
"#;
        std::fs::write(dir.join("profiles/chat.toml"), chat).unwrap();
        registry = build_registry(&dirs);
        assert_eq!(
            registry
                .profile("chat")
                .unwrap()
                .configuration
                .runtime
                .companions["openwebui"]
                .build
                .as_deref(),
            Some("0.11.4")
        );
        let mut settings = Settings::default();
        settings.set_chosen_engine("ollama");
        save(
            &dirs,
            &registry,
            "chat",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = build_registry(&dirs);
        let saved = &registry.profile("chat").unwrap().configuration;
        assert_eq!(
            saved.runtime.companions["openwebui"].build.as_deref(),
            Some("0.11.4")
        );
        assert_eq!(saved.needs.len(), 1);
        // and an export from the store says all of it
        let (text, _) = export_text(&registry, "chat", None, None, &host()).unwrap();
        assert!(
            text.contains("[runtime.openwebui]") && text.contains("[[needs]]"),
            "{text}"
        );
    }

    #[test]
    fn p3_a45_save_save_as_rename_duplicate_delete_and_reset_follow_their_rules() {
        let dir = scratch("a45");
        let dirs = dirs_in(&dir);
        let mut registry = build_registry(&dirs);
        let mut settings = Settings::default();
        let reload = |dirs: &ConfigDirs| build_registry(dirs);

        // Save as: a name, a new configuration; the Default is left as it was
        settings.port = 8099;
        let id = save_as(
            &dirs,
            &registry,
            "Fast chat",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = reload(&dirs);
        assert_eq!(id, "fast-chat");
        let saved = registry.profile("fast-chat").unwrap();
        assert_eq!(Origin::of(saved.layer), Origin::Yours);
        assert_eq!(
            saved.configuration.values["llamacpp.port"],
            Value::Int(8099)
        );
        assert!(
            registry
                .profile(DEFAULT_ID)
                .unwrap()
                .configuration
                .values
                .is_empty(),
            "the one left behind"
        );
        assert_eq!(
            saved.configuration.meta.made_with.as_deref().unwrap(),
            format!("QuickAI {}", env!("CARGO_PKG_VERSION"))
        );
        // an empty name and a name in use are refused (in any case)
        assert!(matches!(
            save_as(
                &dirs,
                &registry,
                "  ",
                captured_now(&registry, &settings),
                &host()
            ),
            Err(StoreError::BadName(_))
        ));
        assert!(matches!(
            save_as(
                &dirs,
                &registry,
                "FAST CHAT",
                captured_now(&registry, &settings),
                &host()
            ),
            Err(StoreError::Taken(_))
        ));
        assert!(matches!(
            save_as(
                &dirs,
                &registry,
                "default",
                captured_now(&registry, &settings),
                &host()
            ),
            Err(StoreError::Taken(_))
        ));

        // Save: the whole file, from the state
        settings.port = 8100;
        save(
            &dirs,
            &registry,
            "fast-chat",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = reload(&dirs);
        assert_eq!(
            registry.profile("fast-chat").unwrap().configuration.values["llamacpp.port"],
            Value::Int(8100)
        );
        assert_eq!(
            registry.profile("fast-chat").unwrap().configuration.name,
            "Fast chat",
            "the name stays"
        );
        assert!(matches!(
            save(
                &dirs,
                &registry,
                "nothing",
                captured_now(&registry, &settings),
                &host()
            ),
            Err(StoreError::Missing(_))
        ));

        // Rename changes the name and nothing else
        let before = std::fs::read_to_string(dir.join("profiles/fast-chat.toml")).unwrap();
        rename(&dirs, &registry, "fast-chat", "Quick chat").unwrap();
        registry = reload(&dirs);
        let after = std::fs::read_to_string(dir.join("profiles/fast-chat.toml")).unwrap();
        assert_eq!(
            after,
            before.replace("name = \"Fast chat\"", "name = \"Quick chat\"")
        );
        assert_eq!(
            registry.profile("fast-chat").unwrap().configuration.name,
            "Quick chat"
        );
        assert!(matches!(
            rename(&dirs, &registry, "fast-chat", "Default"),
            Err(StoreError::Taken(_))
        ));

        // Duplicate asks a name
        let copy = duplicate(&dirs, &registry, "fast-chat", "Quick chat copy").unwrap();
        registry = reload(&dirs);
        assert_eq!(copy, "quick-chat-copy");
        assert!(registry
            .profile(&copy)
            .unwrap()
            .configuration
            .same_run(&registry.profile("fast-chat").unwrap().configuration));
        assert!(matches!(
            duplicate(&dirs, &registry, "fast-chat", "quick chat"),
            Err(StoreError::Taken(_))
        ));

        // Delete: the person's own; the Default cannot be, and the built-in ones are not theirs
        settings.set_active_configuration(&copy);
        delete(&dirs, &registry, &copy).unwrap();
        assert!(forget_if_active(&mut settings, &copy));
        assert_eq!(settings.active_configuration(), None);
        assert!(!forget_if_active(&mut settings, &copy));
        registry = reload(&dirs);
        assert!(registry.profile(&copy).is_none());
        assert!(matches!(
            delete(&dirs, &registry, DEFAULT_ID),
            Err(StoreError::NotYours(_))
        ));
        assert!(matches!(
            delete(&dirs, &registry, "missing"),
            Err(StoreError::Missing(_))
        ));

        // Save over the Default: the person's own stands in for the built-in one, and Reset brings it back
        settings.port = 8200;
        save(
            &dirs,
            &registry,
            DEFAULT_ID,
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        registry = reload(&dirs);
        let default = registry.profile(DEFAULT_ID).unwrap();
        assert_eq!(Origin::of(default.layer), Origin::Yours);
        assert_eq!(
            default.configuration.values["llamacpp.port"],
            Value::Int(8200)
        );
        let text = std::fs::read_to_string(&default.file).unwrap();
        assert!(text.contains("replace = true"), "{text}");
        assert!(
            registry.diagnostics.iter().all(|d| !d.is_error()),
            "{:?}",
            registry.diagnostics
        );
        reset_to_factory(&dirs, &registry).unwrap();
        registry = reload(&dirs);
        let default = registry.profile(DEFAULT_ID).unwrap();
        assert_eq!(Origin::of(default.layer), Origin::BuiltIn);
        assert!(default.configuration.values.is_empty());
        reset_to_factory(&dirs, &registry).unwrap();

        // the list: the Default first, then by name
        let names: Vec<String> = entries(&registry).iter().map(|e| e.name.clone()).collect();
        assert_eq!(names, ["Default", "Quick chat"]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a46_a_folder_that_cannot_be_used_is_said_and_nothing_else_changes() {
        let dir = scratch("a46");
        let dirs = dirs_in(&dir);
        // the folder for the files is a file
        std::fs::write(dir.join(PROFILES_DIR), "not a folder").unwrap();
        let registry = build_registry(&dirs);
        assert!(
            registry
                .diagnostics
                .iter()
                .any(|d| d.message.contains("could not read the directory")),
            "{:?}",
            registry.diagnostics
        );
        assert_eq!(entries(&registry).len(), 1, "the Default is still there");
        let settings = Settings::default();
        let error = save_as(
            &dirs,
            &registry,
            "Mine",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap_err();
        let said = error.to_string();
        assert!(matches!(error, StoreError::Io { .. }), "{said}");
        assert!(
            said.contains(&dir.join(PROFILES_DIR).display().to_string()),
            "{said}"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join(PROFILES_DIR)).unwrap(),
            "not a folder"
        );
        assert!(settings.active_configuration().is_none());
        // no folder at all
        let none = ConfigDirs::default();
        assert_eq!(write_dir(&none), None);
        let registry = build_registry(&none);
        let e = save_as(
            &none,
            &registry,
            "Mine",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap_err();
        assert_eq!(e, StoreError::NoFolder);
        // a file of the folder that cannot be read is listed with the reason, and the others load
        let dirs = dirs_in(&scratch("a46b"));
        let folder = dirs.user.as_ref().unwrap().join(PROFILES_DIR);
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(
            folder.join("broken.toml"),
            "schema = 1\nkind = \"profile\"\nid = \"broken\"\n",
        )
        .unwrap();
        std::fs::write(
            folder.join("later.toml"),
            "schema = 1\nkind = \"profile\"\nid = \"later\"\nrequires = { app = \">=9.0\" }\nengine = \"llamacpp\"\n",
        )
        .unwrap();
        let registry = build_registry(&dirs);
        let bad = unusable(&registry);
        assert_eq!(bad.len(), 2, "{bad:?}");
        assert!(bad
            .iter()
            .any(|b| b.file.ends_with("broken.toml") && b.why.contains("missing field `engine`")));
        assert!(bad
            .iter()
            .any(|b| b.file.ends_with("later.toml") && b.why.contains("update QuickAI")));
        assert!(registry.profile(DEFAULT_ID).is_some());
        let _ = std::fs::remove_dir_all(dirs.user.unwrap());
    }

    #[test]
    fn p3_a42_the_one_new_key_is_absent_for_the_default_and_the_rest_of_the_file_is_as_it_was() {
        let mut settings = Settings::default();
        let nine = serde_json::to_string_pretty(&settings).unwrap();
        // the Default is what a file with no choice means: the file does not change
        settings.set_active_configuration(DEFAULT_ID);
        assert_eq!(settings.active_configuration(), None);
        assert_eq!(serde_json::to_string_pretty(&settings).unwrap(), nine);
        // another one is one key under `values`, and everything else stays
        settings.set_active_configuration("vision-chat");
        assert_eq!(
            settings.active_configuration().as_deref(),
            Some("vision-chat")
        );
        let json: serde_json::Value = serde_json::to_value(&settings).unwrap();
        assert_eq!(
            json["values"],
            serde_json::json!({"app.configuration": "vision-chat"})
        );
        for key in [
            "port",
            "ctx_size",
            "n_gpu_layers",
            "extra_args",
            "dark_mode",
        ] {
            assert_eq!(
                json[key],
                serde_json::to_value(Settings::default()).unwrap()[key],
                "{key}"
            );
        }
        // the engine, the consents and the other values live beside it
        settings.set_chosen_engine("ollama");
        settings.confirm_engine("abc");
        settings.set_active_configuration(DEFAULT_ID);
        let json: serde_json::Value = serde_json::to_value(&settings).unwrap();
        assert!(json["values"].get("app.configuration").is_none());
        assert_eq!(json["values"]["app.engine"], "ollama");
        assert_eq!(
            json["values"]["app.confirmed_engines"],
            serde_json::json!(["abc"])
        );
    }

    #[test]
    fn p3_a43_starting_applies_nothing_and_a_gone_configuration_falls_back_to_the_default() {
        let dir = scratch("a43");
        let dirs = dirs_in(&dir);
        let registry = build_registry(&dirs);
        let mut settings = Settings {
            port: 8123,
            ..Settings::default()
        };
        // nothing chosen: the Default, no sentence
        assert_eq!(
            active_for_start(&registry, &settings),
            (DEFAULT_ID.to_string(), None)
        );
        // a configuration there is
        let id = save_as(
            &dirs,
            &registry,
            "Mine",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        let registry = build_registry(&dirs);
        settings.set_active_configuration(&id);
        assert_eq!(active_for_start(&registry, &settings), (id.clone(), None));
        // one that is gone: the Default, with a sentence for the log, and the settings are as they were
        std::fs::remove_file(dir.join("profiles/mine.toml")).unwrap();
        let registry = build_registry(&dirs);
        let before = serde_json::to_string(&settings).unwrap();
        let (active, log) = active_for_start(&registry, &settings);
        assert_eq!(active, DEFAULT_ID);
        let log = log.expect("a sentence");
        assert!(
            log.contains("`mine`") && log.contains("The settings are as they were"),
            "{log}"
        );
        assert_eq!(serde_json::to_string(&settings).unwrap(), before);
        assert_eq!(settings.port, 8123);
        // a safe-mode start ignores the person's layer
        let safe = ConfigDirs {
            safe_mode: true,
            ..dirs.clone()
        };
        let id = save_as(
            &dirs,
            &build_registry(&dirs),
            "Mine",
            captured_now(&registry, &settings),
            &host(),
        )
        .unwrap();
        settings.set_active_configuration(&id);
        let (active, log) = active_for_start(&build_registry(&safe), &settings);
        assert_eq!(active, DEFAULT_ID);
        assert!(log.is_some());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn p3_a44_the_first_start_adopts_the_working_state_once() {
        // a person who tuned their settings: the state is written as their Default, once
        let dir = scratch("a44");
        let dirs = dirs_in(&dir);
        let registry = build_registry(&dirs);
        let mut settings = Settings {
            port: 8081,
            extra_args: "--threads 8".into(),
            ..Settings::default()
        };
        let written = adopt(
            &dirs,
            &registry,
            &settings,
            &Installed::new(),
            &host(),
            None,
        )
        .unwrap();
        let path = written.expect("a file");
        assert_eq!(path, dir.join("profiles/default.toml"));
        let registry = build_registry(&dirs);
        assert!(
            registry.diagnostics.iter().all(|d| !d.is_error()),
            "{:?}",
            registry.diagnostics
        );
        let adopted = registry.profile(DEFAULT_ID).unwrap();
        assert_eq!(Origin::of(adopted.layer), Origin::Yours);
        assert_eq!(adopted.configuration.name, "Default");
        assert_eq!(
            adopted.configuration.values["llamacpp.port"],
            Value::Int(8081)
        );
        assert_eq!(
            adopted.configuration.values["llamacpp.extra_args"],
            Value::Str("--threads 8".into())
        );
        // so there is nothing unsaved on the first morning
        let now = captured_now(&registry, &settings).configuration;
        assert!(now.same_run(&adopted.configuration));
        // and it is not done again: not when the settings move, not when the file is gone
        settings.port = 9000;
        assert_eq!(
            adopt(
                &dirs,
                &registry,
                &settings,
                &Installed::new(),
                &host(),
                None
            )
            .unwrap(),
            None
        );
        std::fs::remove_file(&path).unwrap();
        assert_eq!(
            adopt(
                &dirs,
                &build_registry(&dirs),
                &settings,
                &Installed::new(),
                &host(),
                None
            )
            .unwrap(),
            None
        );
        assert!(!path.exists());
        let _ = std::fs::remove_dir_all(&dir);

        // a person whose settings are what a first run holds: nothing is written, and the folder
        // that remembers it was done is made
        let dir = scratch("a44b");
        let dirs = dirs_in(&dir);
        let registry = build_registry(&dirs);
        assert_eq!(
            adopt(
                &dirs,
                &registry,
                &Settings::default(),
                &Installed::new(),
                &host(),
                None
            )
            .unwrap(),
            None
        );
        assert_eq!(files_in(&dir.join(PROFILES_DIR)), Vec::<String>::new());
        let later = Settings {
            port: 8200,
            ..Settings::default()
        };
        assert_eq!(
            adopt(
                &dirs,
                &build_registry(&dirs),
                &later,
                &Installed::new(),
                &host(),
                None
            )
            .unwrap(),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    fn inventory_with(engines: &[(&str, EngineHere)]) -> Inventory {
        Inventory {
            app_version: env!("CARGO_PKG_VERSION").into(),
            os: "linux".into(),
            arch: "x86_64".into(),
            ram_bytes: 32 * 1024 * 1024 * 1024,
            engines: engines
                .iter()
                .map(|(id, here)| ((*id).to_string(), here.clone()))
                .collect(),
            ..Inventory::default()
        }
    }

    fn vision() -> Configuration {
        let registry = build_registry(&ConfigDirs::default());
        let text = "schema = 1\nkind = \"profile\"\nid = \"vision\"\nname = \"Vision\"\nengine = \"ollama\"\n\
            [runtime]\nbuild = \"v0.40.0\"\n[[model]]\nollama = \"qwen3-vl:8b\"\nsize = 6140000000\n\
            [values.ollama]\nctx_size = 16384\n[values.openwebui]\nenabled = true\n";
        open_text(&registry, text, env!("CARGO_PKG_VERSION"))
            .unwrap()
            .configuration
    }

    fn a_build(label: &str) -> InstalledBuild {
        InstalledBuild {
            label: label.into(),
            binary_path: PathBuf::from(format!("/data/ollama/{label}/bin/ollama")),
            install_dir: PathBuf::from(format!("/data/ollama/{label}")),
            size_bytes: 1,
        }
    }

    #[test]
    fn choosing_sets_the_engine_the_model_the_build_and_the_settings_and_starts_nothing() {
        let registry = build_registry(&ConfigDirs::default());
        let mut settings = Settings {
            dark_mode: false,
            models_dir_override: Some("/data/models".into()),
            ..Settings::default()
        };
        let installed: Installed = [(
            "ollama".to_string(),
            vec![a_build("v0.40.0"), a_build("v0.35.1")],
        )]
        .into();
        let here = inventory_with(&[
            (
                "ollama",
                EngineHere {
                    installed: true,
                    running: false,
                    builds: vec![
                        BuildHere {
                            label: "v0.40.0".into(),
                            warning: None,
                        },
                        BuildHere {
                            label: "v0.35.1".into(),
                            warning: None,
                        },
                    ],
                    models: ModelsHere::Named {
                        listed: Some(vec![NamedHere {
                            name: "qwen3-vl:8b".into(),
                            size: Some(6_140_000_000),
                        }]),
                    },
                },
            ),
            (
                "openwebui",
                EngineHere {
                    installed: true,
                    ..EngineHere::default()
                },
            ),
        ]);
        let choice =
            choose(&registry, &settings, &vision(), &here, &installed).expect("it can be used");
        assert!(choice.report.stop().is_none());
        let state = &choice.state;
        assert_eq!(state.engine, "ollama");
        assert_eq!(state.values["ollama.ctx_size"], Value::Int(16384));
        assert_eq!(
            state.values["ollama.last_model"],
            Value::Str("qwen3-vl:8b".into())
        );
        assert_eq!(state.values["openwebui.enabled"], Value::Bool(true));
        assert_eq!(state.build.as_deref(), Some("v0.40.0"));
        assert_eq!(
            state.values["ollama.bin_override"],
            Value::Str("/data/ollama/v0.40.0/bin/ollama".into()),
            "the program is the installed build of that label"
        );
        // stored: the engine, the settings, and nothing the configuration does not carry
        store_state(&registry, &mut settings, state).unwrap();
        settings.set_active_configuration("vision");
        assert_eq!(settings.chosen_engine().as_deref(), Some("ollama"));
        assert_eq!(settings.active_configuration().as_deref(), Some("vision"));
        assert!(!settings.dark_mode);
        assert_eq!(
            settings.models_dir_override.as_deref(),
            Some("/data/models")
        );
        let engine = registry.engine("ollama").unwrap();
        assert_eq!(
            settings.engine_values(engine)["ollama.ctx_size"],
            Value::Int(16384)
        );
        // what the working state now is, is what the configuration describes
        let back = working_state(&registry, &settings, &installed);
        assert_eq!(back.build.as_deref(), Some("v0.40.0"));
        assert!(
            capture(&registry, &back, &first_run_values(&registry), None)
                .configuration
                .same_run(&vision())
        );
    }

    #[test]
    fn choosing_a_configuration_of_another_engine_keeps_that_engines_folders_and_programs() {
        let registry = build_registry(&ConfigDirs::default());
        let llama = registry.engine("llamacpp").unwrap();
        let ollama = registry.engine("ollama").unwrap();
        // a llama.cpp with a folder and a program of the person's, and an Ollama with a store of its own and a
        // context size the Default does not carry; Ollama is the engine in use
        let mut settings = Settings {
            models_dir_override: Some("/my/models".into()),
            llama_bin_override: Some("/my/llama-server".into()),
            ..Settings::default()
        };
        let ollama_values: BTreeMap<String, Value> = [
            ("ollama.models_dir", Value::Str("/my/ollama-store".into())),
            ("ollama.bin_override", Value::Str("/my/ollama".into())),
            ("ollama.ctx_size", Value::Int(8192)),
        ]
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
        settings.set_engine_values(ollama, &ollama_values).unwrap();
        settings.set_chosen_engine("ollama");
        let installed = Installed::default();
        let here = inventory_with(&[]);

        // back to llama.cpp's Default (what Revert does after the engine was changed): its folder and program stay
        let default = registry
            .profile(DEFAULT_ID)
            .expect("the built-in Default")
            .configuration
            .clone();
        let choice =
            choose(&registry, &settings, &default, &here, &installed).expect("it can be used");
        store_state(&registry, &mut settings, &choice.state).unwrap();
        assert_eq!(
            settings.chosen_engine(),
            None,
            "llama.cpp is the absence of a choice"
        );
        assert_eq!(settings.models_dir_override.as_deref(), Some("/my/models"));
        assert_eq!(
            settings.llama_bin_override.as_deref(),
            Some("/my/llama-server")
        );
        assert!(settings
            .engine_values(llama)
            .contains_key("llamacpp.models_dir"));
        // and Ollama's values were not touched
        let kept = settings.engine_values(ollama);
        assert_eq!(
            kept["ollama.models_dir"],
            Value::Str("/my/ollama-store".into())
        );
        assert_eq!(kept["ollama.bin_override"], Value::Str("/my/ollama".into()));

        // to an Ollama configuration from llama.cpp: Ollama's store and program ride along, and what the
        // configuration can carry and does not give returns to its default
        let choice =
            choose(&registry, &settings, &vision(), &here, &installed).expect("it can be used");
        store_state(&registry, &mut settings, &choice.state).unwrap();
        assert_eq!(settings.chosen_engine().as_deref(), Some("ollama"));
        let now = settings.engine_values(ollama);
        assert_eq!(
            now["ollama.models_dir"],
            Value::Str("/my/ollama-store".into())
        );
        assert_eq!(now["ollama.bin_override"], Value::Str("/my/ollama".into()));
        assert_eq!(
            now["ollama.ctx_size"],
            Value::Int(16384),
            "the configuration's own"
        );
        assert_eq!(settings.models_dir_override.as_deref(), Some("/my/models"));
        assert_eq!(
            settings.llama_bin_override.as_deref(),
            Some("/my/llama-server")
        );
    }

    #[test]
    fn choosing_a_model_that_is_not_here_leaves_none_selected_and_says_so() {
        let registry = build_registry(&ConfigDirs::default());
        let settings = Settings::default();
        // Ollama is running and has other models: the model is missing and can be got
        let here = inventory_with(&[(
            "ollama",
            EngineHere {
                installed: true,
                running: true,
                builds: vec![BuildHere {
                    label: "v0.40.0".into(),
                    warning: None,
                }],
                models: ModelsHere::Named {
                    listed: Some(vec![NamedHere {
                        name: "other:1b".into(),
                        size: None,
                    }]),
                },
            },
        )]);
        let installed: Installed = [("ollama".to_string(), vec![a_build("v0.40.0")])].into();
        let choice = choose(&registry, &settings, &vision(), &here, &installed).unwrap();
        assert!(!choice.state.values.contains_key("ollama.last_model"));
        assert!(choice.report.has("model_to_get"));
        // a build that is not installed is told, and the program is left as it was
        let here = inventory_with(&[(
            "ollama",
            EngineHere {
                installed: true,
                models: ModelsHere::Named { listed: None },
                ..EngineHere::default()
            },
        )]);
        let choice = choose(&registry, &settings, &vision(), &here, &Installed::new()).unwrap();
        assert!(choice.report.has("build_none"));
        assert_eq!(choice.state.build, None);
        assert!(!choice.state.values.contains_key("ollama.bin_override"));
        // a configuration for an engine this version lacks stops, and nothing is made
        let mut alien = vision();
        alien.engine = "whispercpp".into();
        let report = choose(&registry, &settings, &alien, &here, &Installed::new()).unwrap_err();
        assert_eq!(report.stop().unwrap().id, "unknown_engine");
    }

    #[test]
    fn p3_a60_a_model_by_name_is_looked_for_in_the_list_its_engine_gave_last_time() {
        let dir = scratch("a60");
        let dirs = dirs_in(&dir);
        let registry = build_registry(&dirs);
        let facts = quickai_core::plan::HostFacts {
            data_dir: dir.join("data").display().to_string(),
            ..quickai_core::plan::HostFacts::default()
        };
        // the engine has never listed anything: nothing is known, and it is not said to be missing
        assert!(!listed_by_engines(&registry, &facts).contains_key("ollama"));
        let configuration = vision();
        let here = |listed: &BTreeMap<String, Vec<NamedHere>>, running: bool| {
            let models = models_here(
                &registry,
                &dir.join("bundled"),
                &|_| dir.join("models"),
                listed,
            );
            let mut up = BTreeSet::new();
            if running {
                up.insert("ollama".to_string());
            }
            inventory(
                &registry,
                &host(),
                &Installed::new(),
                &models,
                Now {
                    running: &up,
                    busy_ports: BTreeSet::new(),
                    first_addresses: BTreeMap::new(),
                },
                &|_| true,
            )
        };
        let ids = |inv: &Inventory| -> Vec<&'static str> {
            review(&registry, &configuration, &[], inv, false)
                .notes
                .iter()
                .map(|n| n.id)
                .filter(|id| id.starts_with("model_"))
                .collect()
        };
        assert_eq!(
            ids(&here(&listed_by_engines(&registry, &facts), false)),
            ["model_unseen"]
        );

        // what it listed last time is in its cache
        let cache = dir.join("data/ollama/catalog-cache.json");
        std::fs::create_dir_all(cache.parent().unwrap()).unwrap();
        let rows = |names: &[&str]| {
            serde_json::json!({"version": 1, "rows": names.iter().map(|n| serde_json::json!({
                "id": n, "display": n, "size_bytes": 6_140_000_000u64, "digest": "d", "capabilities": []
            })).collect::<Vec<_>>()})
            .to_string()
        };
        std::fs::write(&cache, rows(&["qwen3-vl:8b", "other:1b"])).unwrap();
        let listed = listed_by_engines(&registry, &facts);
        assert_eq!(listed["ollama"].len(), 2);
        assert_eq!(listed["ollama"][0].size, Some(6_140_000_000));
        // it is there: nothing to say, whether or not the engine runs
        assert!(ids(&here(&listed, false)).is_empty());
        assert!(ids(&here(&listed, true)).is_empty());
        // it is not in the list: stopped, that cannot be told and is never said to be missing; running, it is missing
        std::fs::write(&cache, rows(&["other:1b"])).unwrap();
        let listed = listed_by_engines(&registry, &facts);
        assert_eq!(ids(&here(&listed, false)), ["model_unseen"]);
        assert_eq!(ids(&here(&listed, true)), ["model_to_get"]);
        // a cache that is not ours is not believed
        std::fs::write(&cache, "{\"version\": 9, \"rows\": []}").unwrap();
        assert!(!listed_by_engines(&registry, &facts).contains_key("ollama"));
        // what the engine says is its own bookkeeping is not a model
        std::fs::write(&cache, rows(&["llamacpp:abc123", "qwen3-vl:8b"])).unwrap();
        let listed = listed_by_engines(&registry, &facts);
        assert_eq!(
            listed["ollama"]
                .iter()
                .map(|m| m.name.as_str())
                .collect::<Vec<_>>(),
            ["qwen3-vl:8b"]
        );
        // choosing sets the name when it is there, and clears it when the engine runs and has none
        let settings = Settings::default();
        let choice = choose(
            &registry,
            &settings,
            &configuration,
            &here(&listed, false),
            &Installed::new(),
        )
        .unwrap();
        assert_eq!(
            choice.state.values["ollama.last_model"],
            Value::Str("qwen3-vl:8b".into())
        );
        std::fs::write(&cache, rows(&["other:1b"])).unwrap();
        let listed = listed_by_engines(&registry, &facts);
        let choice = choose(
            &registry,
            &settings,
            &configuration,
            &here(&listed, true),
            &Installed::new(),
        )
        .unwrap();
        assert!(!choice.state.values.contains_key("ollama.last_model"));
    }
}
