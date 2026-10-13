//! What a configuration needs on this computer, as steps (docs/headless.md, section 4).
//!
//! The review of a configuration says what differs on this computer; this turns the part of it that can
//! be put right into things to do, in the order they are done: install the programs that are missing,
//! fetch the files of the models, and then (when the programs run) start them and pull and load what an
//! engine pulls by name. Planning reads the disk and, to name a build that nobody named, asks the engine's
//! feed what it lists; it installs, fetches and starts nothing.
//!
//! Where a file comes from is the configuration's to say (a place and a name in it, an address, an entry of
//! the recommended list) and the person's to change: a file they already have is used where it is, an
//! address or a repository they name is fetched from instead, a folder they point at is looked in first,
//! and a mirror replaces the hub (docs/headless.md, section 7).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use quickai_core::lookup::huggingface::file_url;
use quickai_core::lookup::present::{safe_file_name, Download, Fetch, Piece};
use quickai_core::plan::HostFacts;
use quickai_core::profile::check::human_bytes;
use quickai_core::profile::environment::{Environment, FileHint, Where};
use quickai_core::profile::state::Applied;
use quickai_core::profile::{url_label, Configuration, ModelPick, ModelSource};
use quickai_core::registry::{Engine, Registry};
use quickai_core::value::Value;

use super::build;
use super::report::{Failure, FailureKind, FileInfo, StepInfo};
use super::Options;
use crate::config::ConfigDirs;
use crate::configurations::runtime_field;
use crate::downloader::FetchWith;
use crate::gpu::GpuVendor;
use crate::launch;
use crate::prims::install::{self, InstallRequest, InstallSpec};
use crate::resolve;

/// Everything planning reads.
pub(super) struct Ctx<'a> {
    pub registry: &'a Registry,
    pub dirs: &'a ConfigDirs,
    pub options: &'a Options,
    pub configuration: &'a Configuration,
    pub applied: &'a Applied,
    pub facts: &'a HostFacts,
    pub vendors: &'a [GpuVendor],
    /// The configuration is a file from outside, not one the person keeps.
    pub outside: bool,
    /// What the person said about this computer, put together.
    pub environment: &'a Environment,
}

/// Which program a step is about.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Slot {
    Main,
    Companion,
}

/// What a step does.
pub(super) enum Work {
    Install {
        slot: Slot,
        spec: Box<InstallSpec>,
        request: InstallRequest,
    },
    Fetch {
        download: Download,
        with: FetchWith,
    },
}

pub(super) struct Step {
    pub info: StepInfo,
    pub work: Work,
}

/// The program that goes with the engine, when the configuration has it start.
pub(super) struct Companion {
    pub id: String,
    pub binary: Option<PathBuf>,
}

/// What is to run once the steps are done.
pub(super) struct Intent {
    pub engine: String,
    /// The values of the engine and of the program that goes with it, by `<engine>.<field>`.
    pub values: BTreeMap<String, Value>,
    pub binary: Option<PathBuf>,
    pub companion: Option<Companion>,
    /// The file of the model an engine of files runs, in the models folder.
    pub model_file: Option<String>,
    /// The names an engine that pulls its models has to have; the first is the one it loads.
    pub pulls: Vec<String>,
    pub models_dir: PathBuf,
}

pub(super) struct Plan {
    pub steps: Vec<Step>,
    pub intent: Intent,
    /// What is done when the programs run, for the plan to say.
    pub later: Vec<StepInfo>,
    /// Things worth saying that are not steps: a file that was found and could not be used.
    pub notes: Vec<String>,
}

fn needs(message: impl Into<String>) -> Failure {
    Failure::new(FailureKind::Needs, message)
}

fn mistake(message: impl Into<String>) -> Failure {
    Failure::new(FailureKind::Mistake, message)
}

fn info(kind: &str, text: String, bytes: Option<u64>) -> StepInfo {
    StepInfo {
        kind: kind.to_string(),
        text,
        bytes,
        files: Vec::new(),
    }
}

fn sized(bytes: Option<u64>) -> String {
    bytes.map_or(String::new(), |b| format!(" ({})", human_bytes(b)))
}

/// The program of `engine`: where it is when it is there, else the step that installs it.
fn program(
    ctx: &Ctx<'_>,
    engine: &Engine,
    slot: Slot,
    wanted: Option<&str>,
) -> Result<(Option<PathBuf>, Option<Step>), Failure> {
    let data = ctx.dirs.data.as_ref().map(|d| d.display().to_string());
    let spec = InstallSpec::from_engine(engine, ctx.facts)
        .map(|s| s.with_python(ctx.options.python.clone()));
    let installed = spec
        .as_ref()
        .map(install::list_installed)
        .unwrap_or_default();
    let name = &engine.doc.engine.name;

    // a program the person named themselves (`--set <engine>.bin_override=...`) is the one that is used
    if let Some(field) = runtime_field(engine) {
        if let Some(named) = ctx
            .applied
            .set
            .get(&format!("{}.{field}", engine.id))
            .and_then(Value::as_str)
            .filter(|p| !p.trim().is_empty())
        {
            let path = PathBuf::from(named);
            return if path.is_file() {
                Ok((Some(path), None))
            } else {
                Err(needs(format!(
                    "{name}: the program you named, {named}, is not a file"
                )))
            };
        }
    }

    match wanted {
        Some(label) => {
            if let Some(build) = installed.iter().find(|b| b.label == label) {
                return Ok((Some(build.binary_path.clone()), None));
            }
        }
        None => {
            // what the document finds by itself: a package's, one on the path, an installed build
            let found = launch::locate_binary_in(ctx.registry, &engine.id, None, data.as_deref());
            if found.is_file() {
                return Ok((Some(found), None));
            }
            // an engine whose document does not look in its own folder of builds (llama.cpp) uses the one
            // that was installed last, so that a second run does not install another
            if let Some(build) = installed.first() {
                return Ok((Some(build.binary_path.clone()), None));
            }
        }
    }

    if ctx.options.offline {
        let which = wanted.map_or(String::new(), |label| format!(" {label}"));
        return Err(needs(format!(
            "{name}{which} is not on this computer, and --offline says nothing is fetched. Install it first (`quickai-run prepare` does, with the network), or point to one that is installed with --build or the `bin_override` setting"
        )));
    }
    let Some(spec) = spec else {
        return Err(needs(format!(
            "{name} is not on this computer, and its document has no way to install it{}",
            wanted.map_or(String::new(), |label| format!(
                " (the build {label} was asked for)"
            ))
        )));
    };
    let token = ctx
        .environment
        .fetch
        .github_token
        .as_ref()
        .map(|t| t.expose());
    let chosen = build::choose(engine, ctx.facts, ctx.vendors, wanted, token).map_err(|why| {
        needs(format!(
            "{name} is not on this computer, and which build to install could not be told: {why}"
        ))
    })?;
    let summary = quickai_core::acquire::install_summary(engine, ctx.facts)
        .map(|s| format!(". {s}"))
        .unwrap_or_default();
    let weight = chosen
        .bytes
        .map_or(String::new(), |b| format!("{}; ", human_bytes(b)));
    let text = format!(
        "install {name} {} ({weight}{}){summary}",
        chosen.label, chosen.check
    );
    Ok((
        None,
        Some(Step {
            info: info("install", text, chosen.bytes),
            work: Work::Install {
                slot,
                spec: Box::new(spec),
                request: chosen.request,
            },
        }),
    ))
}

// ---- the files of the models ----------------------------------------------------------------------

/// The folder of the models folder a piece is kept in.
fn folder_of(piece: &Piece, dir: &Path) -> PathBuf {
    match &piece.place {
        Some(place) => dir.join(place),
        None => dir.to_path_buf(),
    }
}

/// Whether a piece of a download is in the models folder already, at the size it should be.
fn is_here(piece: &Piece, dir: &Path) -> bool {
    std::fs::metadata(folder_of(piece, dir).join(&piece.store_as))
        .is_ok_and(|m| m.is_file() && piece.size.is_none_or(|s| s == m.len()))
}

/// The last name of a path in a repository.
fn base(name: &str) -> &str {
    name.rsplit('/').next().unwrap_or(name)
}

/// The hub the files of an engine are fetched from: the site its look-up of Hugging Face names (a mirror
/// the person pointed it at, or the real one).
fn hub_site(engine: &Engine) -> String {
    quickai_core::models::lookups(engine)
        .iter()
        .find_map(|l| match &l.kind {
            quickai_core::registry::engine::LookupKind::HuggingFace { site, .. } => {
                Some(site.trim_end_matches('/').to_string())
            }
            _ => None,
        })
        .unwrap_or_else(|| "https://huggingface.co".to_string())
}

/// Where a file says it can be had from, in full, for the sentence that asks whether to go there.
fn said_source(pick: &ModelPick) -> String {
    if let Some(url) = &pick.url {
        return url.clone();
    }
    let mut text = pick.from.clone().unwrap_or_default();
    match (&pick.path, &pick.revision) {
        (Some(path), Some(revision)) => text.push_str(&format!(" (the file {path} at {revision})")),
        (Some(path), None) => text.push_str(&format!(" (the file {path})")),
        (None, Some(revision)) => text.push_str(&format!(" (at {revision})")),
        (None, None) => {}
    }
    text
}

/// A download of one file of the place its configuration says it can be had from: a repository (at a
/// revision, in a folder of it) or an address. A file that says nothing has no address yet, and may be
/// given one by the person.
fn download_from_source(
    engine: &Engine,
    pick: &ModelPick,
    name: &str,
) -> Result<Download, Failure> {
    let place = pick
        .into
        .clone()
        .or_else(|| quickai_core::models::place_for(engine, name));
    let (url, source, repo_name) = if let Some(url) = &pick.url {
        (url.clone(), url_label(url), name.to_string())
    } else if let Some(from) = &pick.from {
        let repo = from.strip_prefix("huggingface:").ok_or_else(|| {
            needs(format!(
                "{name} is said to come from `{from}`, and only huggingface: is a place this version fetches from"
            ))
        })?;
        let path = pick.path.clone().unwrap_or_else(|| name.to_string());
        let revision = pick.revision.as_deref().unwrap_or("main");
        (
            file_url(&hub_site(engine), repo, revision, &path),
            from.clone(),
            path,
        )
    } else {
        (String::new(), "your own copy".to_string(), name.to_string())
    };
    Ok(Download {
        files: vec![Piece {
            kind: None,
            label: "model".to_string(),
            url,
            store_as: name.to_string(),
            name: repo_name,
            size: pick.size,
            sha256: pick.sha256.clone(),
            place,
        }],
        bundle: None,
        source,
        mode: None,
    })
}

/// What a person who says where a file is can mean, and what the pieces of a download look like to it.
struct Located {
    /// The download, with the address of each piece as it will be fetched.
    download: Download,
    with: FetchWith,
    /// The pieces that are not in the models folder yet, one by one.
    files: Vec<FileInfo>,
    /// Said besides the plan.
    notes: Vec<String>,
    /// Some piece is fetched from the place its configuration names, and nobody said another.
    declared: bool,
}

/// The nearest a piece is in a folder the person pointed at: by the name it is kept under, by its place,
/// by the name it has in its repository, by the path it has there. A file of the wrong size is not it.
fn find_in(piece: &Piece, dirs: &[PathBuf], notes: &mut Vec<String>) -> Option<PathBuf> {
    let mut names: Vec<PathBuf> = Vec::new();
    for name in [
        piece.store_as.as_str(),
        base(&piece.name),
        piece.name.as_str(),
    ] {
        if let Some(place) = &piece.place {
            names.push(Path::new(place).join(name));
        }
        names.push(PathBuf::from(name));
    }
    for dir in dirs {
        for name in &names {
            let candidate = dir.join(name);
            let Ok(meta) = std::fs::metadata(&candidate) else {
                continue;
            };
            if !meta.is_file() {
                continue;
            }
            match piece.size {
                Some(size) if size != meta.len() => notes.push(format!(
                    "{} is {} bytes and {} should be {size}: it is not used",
                    candidate.display(),
                    meta.len(),
                    piece.store_as
                )),
                _ => return Some(candidate),
            }
        }
    }
    None
}

/// The same address on another hub: the part after the repository stays.
fn on_repository(url: &str, site: &str, repo: &str) -> Option<String> {
    let rest = url.strip_prefix(site)?.trim_start_matches('/');
    let (_, tail) = rest.split_once("/resolve/")?;
    Some(format!("{site}/{repo}/resolve/{tail}"))
}

/// What the person said about each piece that is not here: their own file, another address or repository,
/// a folder to look in; else the address its configuration gave, on the mirror if there is one.
fn locate(
    ctx: &Ctx<'_>,
    engine: &Engine,
    mut download: Download,
    dir: &Path,
    known: &mut BTreeSet<String>,
    strict: bool,
) -> Result<Located, Failure> {
    let site = hub_site(engine);
    let endpoint = ctx.environment.fetch.huggingface_endpoint.clone();
    let mut with = FetchWith {
        strict_size: strict,
        ..FetchWith::default()
    };
    if let Some(token) = &ctx.environment.fetch.huggingface_token {
        let hub = endpoint.as_deref().unwrap_or(&site);
        if let Some((_, host, _)) = quickai_core::profile::split_url_with(hub, true) {
            let host = host.split(':').next().unwrap_or(host);
            with.logins.push((host.to_string(), token.clone()));
        }
    }
    let mut files = Vec::new();
    let mut notes = Vec::new();
    let mut declared = false;
    for piece in &mut download.files {
        known.insert(piece.store_as.clone());
        known.insert(base(&piece.name).to_string());
        if is_here(piece, dir) {
            continue;
        }
        let to = folder_of(piece, dir).join(&piece.store_as);
        let said = ctx
            .environment
            .models
            .get(&piece.store_as)
            .or_else(|| ctx.environment.models.get(base(&piece.name)));
        match said {
            Some(Where::Path(path)) => {
                let path = PathBuf::from(path);
                let file = if path.is_dir() {
                    find_in(piece, std::slice::from_ref(&path), &mut notes).ok_or_else(|| {
                        needs(format!(
                            "{} is not in the folder {} that you named for it",
                            piece.store_as,
                            path.display()
                        ))
                    })?
                } else if path.is_file() {
                    path
                } else {
                    return Err(needs(format!(
                        "there is no file at {}, which you named for {}",
                        path.display(),
                        piece.store_as
                    )));
                };
                with.locals.insert(piece.store_as.clone(), file.clone());
                files.push(FileInfo {
                    name: piece.store_as.clone(),
                    how: "link".into(),
                    from: file.display().to_string(),
                    to: to.display().to_string(),
                    bytes: piece
                        .size
                        .or_else(|| std::fs::metadata(&file).ok().map(|m| m.len())),
                    sha256: piece.sha256.clone(),
                });
                continue;
            }
            Some(Where::Url(url)) => piece.url = url.clone(),
            Some(Where::Place(place)) => {
                let repo = place.strip_prefix("huggingface:").ok_or_else(|| {
                    needs(format!(
                        "`{place}` is a place this version does not fetch from; only huggingface: is"
                    ))
                })?;
                piece.url = if piece.url.is_empty() {
                    file_url(&site, repo, "main", &piece.name)
                } else {
                    on_repository(&piece.url, &site, repo)
                        .unwrap_or_else(|| file_url(&site, repo, "main", &piece.name))
                };
            }
            None => {
                if let Some(file) = find_in(piece, &ctx.options.models_from, &mut notes) {
                    with.locals.insert(piece.store_as.clone(), file.clone());
                    files.push(FileInfo {
                        name: piece.store_as.clone(),
                        how: "link".into(),
                        from: file.display().to_string(),
                        to: to.display().to_string(),
                        bytes: piece
                            .size
                            .or_else(|| std::fs::metadata(&file).ok().map(|m| m.len())),
                        sha256: piece.sha256.clone(),
                    });
                    continue;
                }
            }
        }
        if said.is_none() {
            declared = true;
        }
        if piece.url.is_empty() {
            return Err(needs(format!(
                "{} is not in {} and the configuration does not say where to get it; put the file there, name where it is with --model-source {}=PATH, give a folder that has it with --models-from, or name a model of the recommended list",
                piece.store_as,
                folder_of(piece, dir).display(),
                piece.store_as
            )));
        }
        if let Some(endpoint) = &endpoint {
            if let Some(rest) = piece.url.strip_prefix(&site) {
                piece.url = format!("{endpoint}{rest}");
            }
        }
        files.push(FileInfo {
            name: piece.store_as.clone(),
            how: "fetch".into(),
            from: piece.url.clone(),
            to: to.display().to_string(),
            bytes: piece.size,
            sha256: piece.sha256.clone(),
        });
    }
    Ok(Located {
        download,
        with,
        files,
        notes,
        declared,
    })
}

/// The names of a few files, as a sentence has them: the first and how many more.
fn few(names: &[&str]) -> String {
    match names {
        [] => String::new(),
        [only] => (*only).to_string(),
        [first, rest @ ..] => format!(
            "{first} and {} more file{}",
            rest.len(),
            if rest.len() == 1 { "" } else { "s" }
        ),
    }
}

/// The step that gets `located`, unless every file of it is here.
fn fetch_step(located: Located) -> Option<Step> {
    if located.files.is_empty() {
        return None;
    }
    let fetched: Vec<&FileInfo> = located.files.iter().filter(|f| f.how == "fetch").collect();
    let linked: Vec<&FileInfo> = located.files.iter().filter(|f| f.how == "link").collect();
    let bytes: Option<u64> = fetched.iter().map(|f| f.bytes).sum();
    let mut parts = Vec::new();
    if !fetched.is_empty() {
        let names: Vec<&str> = fetched.iter().map(|f| f.name.as_str()).collect();
        parts.push(format!(
            "fetch {}{} from {}",
            few(&names),
            sized(bytes),
            located.download.source
        ));
    }
    if !linked.is_empty() {
        let names: Vec<&str> = linked.iter().map(|f| f.name.as_str()).collect();
        let size: Option<u64> = linked.iter().map(|f| f.bytes).sum();
        parts.push(format!("use {}{} that you have", few(&names), sized(size)));
    }
    let kind = if fetched.is_empty() { "link" } else { "fetch" };
    let mut step_info = info(
        kind,
        parts.join("; "),
        if fetched.is_empty() { None } else { bytes },
    );
    step_info.files = located.files;
    Some(Step {
        info: step_info,
        work: Work::Fetch {
            download: located.download,
            with: located.with,
        },
    })
}

/// A file model, as a download: where it comes from and what the person has said about it. `None` when it
/// is in the models folder already, or when the engine runs without it.
struct Wanted {
    /// The name the engine runs the model by (the weights).
    weights: String,
    download: Option<Download>,
    /// The download is of a file the configuration describes (its size is held to what it says).
    strict: bool,
    /// Where the configuration says the file can be had from, in full, when that is a place the recommended
    /// list does not vouch for: a file from outside is read by the person before it is fetched from there.
    named: Option<String>,
}

/// The downloads the models of the configuration are, for an engine that keeps its models as files. A file
/// from outside that names a place to fetch from asks first, unless the person has said where to get the file
/// from themselves.
fn wanted_downloads(
    ctx: &Ctx<'_>,
    engine: &Engine,
    models_dir: &Path,
    entries: &[quickai_core::lookup::present::Entry],
) -> Result<Vec<Wanted>, Failure> {
    let needs_model = engine.doc.provides.needs_model;
    let folders = quickai_core::models::folders(engine);
    let mut out = Vec::new();
    for pick in &ctx.configuration.models {
        let wanted = match &pick.source {
            ModelSource::Catalog {
                id,
                file: Some(file),
            } => {
                let fetch =
                    quickai_core::models::catalog_fetch(entries, id, file).ok_or_else(|| {
                        needs(format!(
                            "the recommended list has no {file} for {}",
                            engine.doc.engine.name
                        ))
                    })?;
                match fetch {
                    Fetch::Files(download) => Wanted {
                        weights: file.clone(),
                        download: Some(download),
                        strict: false,
                        named: None,
                    },
                    Fetch::Reference(_) => {
                        return Err(needs(format!("{file} is not a file to fetch")))
                    }
                }
            }
            ModelSource::Catalog { id, file: None } => {
                return Err(needs(format!(
                    "the configuration names the model `{id}` of the recommended list and not which of its files; add `file = \"...\"`"
                )))
            }
            ModelSource::File { name } => {
                if safe_file_name(name) != *name {
                    return Err(needs(format!(
                        "{name}: this version keeps files under names of letters, digits, `.`, `-` and `_`"
                    )));
                }
                if let Some(into) = &pick.into {
                    if !folders.contains(into) {
                        return Err(needs(if folders.is_empty() {
                            format!(
                                "{name} is to go in the folder `{into}`, and {} keeps all its files in one folder",
                                engine.doc.engine.name
                            )
                        } else {
                            format!(
                                "{name} is to go in the folder `{into}`, and {} keeps its files in {}",
                                engine.doc.engine.name,
                                folders.join(", ")
                            )
                        }));
                    }
                }
                let place = pick
                    .into
                    .clone()
                    .or_else(|| quickai_core::models::place_for(engine, name));
                let here = place
                    .as_ref()
                    .map_or_else(|| models_dir.to_path_buf(), |place| models_dir.join(place))
                    .join(name);
                let said = ctx.environment.models.contains_key(name)
                    || ctx
                        .options
                        .models_from
                        .iter()
                        .any(|dir| dir.join(name).is_file() || place.as_ref().is_some_and(|p| dir.join(p).join(name).is_file()));
                if here.is_file() {
                    Wanted {
                        weights: name.clone(),
                        download: None,
                        strict: false,
                        named: None,
                    }
                } else if pick.from.is_some() || pick.url.is_some() || said {
                    // a file from outside that names a place: whether it is fetched from there is decided when
                    // it is known what the person said about the file (`make` asks then)
                    Wanted {
                        weights: name.clone(),
                        download: Some(download_from_source(engine, pick, name)?),
                        strict: true,
                        named: (pick.from.is_some() || pick.url.is_some()).then(|| said_source(pick)),
                    }
                } else if needs_model {
                    return Err(needs(format!(
                        "{name} is not in {} and the configuration does not say where to get it; put the file there, name where it is with --model-source {name}=PATH, give a folder that has it with --models-from, or name a model of the recommended list",
                        models_dir.display()
                    )));
                } else {
                    Wanted {
                        weights: name.clone(),
                        download: None,
                        strict: false,
                        named: None,
                    }
                }
            }
            ModelSource::Named { name, .. } => {
                return Err(needs(format!(
                    "`{name}` is a name {} pulls, and {} keeps its models as files",
                    "an engine that keeps its own models",
                    engine.doc.engine.name
                )))
            }
        };
        out.push(wanted);
    }
    Ok(out)
}

/// The files the configuration fetches, for the file the person fills in: their names, where each comes from
/// and how big it is. It looks at no disk and asks no one.
pub(super) fn file_hints(ctx: &Ctx<'_>) -> Result<Vec<FileHint>, Failure> {
    let registry = ctx.registry;
    let engine = registry.engine(&ctx.configuration.engine).ok_or_else(|| {
        Failure::new(
            FailureKind::Unreadable,
            format!("there is no engine called `{}`", ctx.configuration.engine),
        )
    })?;
    if engine.models.as_ref().is_some_and(|m| m.list.is_some()) {
        return Ok(Vec::new());
    }
    let entries = quickai_core::models::recommended(registry, engine);
    // a folder that holds nothing: every file of the configuration is one to get
    let nowhere = PathBuf::from("/nonexistent/quickai-models");
    let mut hints = Vec::new();
    for wanted in wanted_downloads(ctx, engine, &nowhere, &entries)? {
        if let Some(download) = wanted.download {
            for piece in &download.files {
                let source = if piece.url.is_empty() {
                    download.source.clone()
                } else {
                    format!(
                        "{}{}",
                        download.source,
                        if piece.name != piece.store_as && !piece.name.is_empty() {
                            format!(" ({})", piece.name)
                        } else {
                            String::new()
                        }
                    )
                };
                hints.push(FileHint {
                    name: piece.store_as.clone(),
                    source,
                    size: piece.size,
                });
            }
        }
    }
    Ok(hints)
}

/// The steps a configuration needs on this computer, and what is to run after them.
pub(super) fn make(ctx: &Ctx<'_>) -> Result<Plan, Failure> {
    let registry = ctx.registry;
    let engine = registry.engine(&ctx.configuration.engine).ok_or_else(|| {
        Failure::new(
            FailureKind::Unreadable,
            format!("there is no engine called `{}`", ctx.configuration.engine),
        )
    })?;
    let key = |field: &str| format!("{}.{field}", engine.id);

    let mut values = ctx.applied.set.clone();
    if let Some(port) = ctx.options.port {
        values.insert(key("port"), Value::from(port));
    }
    let own_text = |values: &BTreeMap<String, Value>, field: &str| {
        values
            .get(&key(field))
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .map(str::to_string)
    };
    let models_dir = ctx
        .options
        .models_dir
        .clone()
        .or_else(|| own_text(&values, "models_dir").map(PathBuf::from))
        .or_else(|| {
            let mut selection = quickai_core::plan::Selection::new(&engine.id);
            selection.values = values.clone();
            quickai_core::models::files_dir(engine, &selection, ctx.facts).map(PathBuf::from)
        })
        .unwrap_or_else(|| resolve::models_dirs(ctx.dirs).1);
    values.insert(
        key("models_dir"),
        Value::Str(models_dir.display().to_string()),
    );

    let mut steps = Vec::new();
    let mut later = Vec::new();
    let mut notes = Vec::new();

    // the program of the engine, and of the one that goes with it
    let wanted = ctx
        .options
        .build
        .as_deref()
        .or(ctx.configuration.runtime.build.as_deref());
    let (binary, step) = program(ctx, engine, Slot::Main, wanted)?;
    steps.extend(step);
    if let (Some(binary), Some(field)) = (&binary, runtime_field(engine)) {
        values.insert(key(field), Value::Str(binary.display().to_string()));
    }

    let mut companion = None;
    for member in registry.companions_of(&engine.id) {
        let enabled_key = format!("{}.enabled", member.id);
        let enabled = match values.get(&enabled_key) {
            Some(value) => value.truthy(),
            None => member
                .doc
                .fields
                .iter()
                .find(|f| f.id == "enabled")
                .and_then(|f| match &f.default {
                    Some(quickai_core::doc::engine::FieldDefault::Literal(v)) => v.as_bool(),
                    _ => None,
                })
                .unwrap_or(false),
        };
        if !enabled {
            continue;
        }
        // the build the person named, else the one the configuration pins, else the document's choice
        let pinned = ctx
            .options
            .companion_builds
            .get(&member.id)
            .map(String::as_str)
            .or_else(|| {
                ctx.configuration
                    .runtime
                    .companions
                    .get(&member.id)
                    .and_then(|r| r.build.as_deref())
            });
        let (binary, step) = program(ctx, member, Slot::Companion, pinned)?;
        steps.extend(step);
        if let (Some(binary), Some(field)) = (&binary, runtime_field(member)) {
            values.insert(
                format!("{}.{field}", member.id),
                Value::Str(binary.display().to_string()),
            );
        }
        companion = Some(Companion {
            id: member.id.clone(),
            binary,
        });
        break;
    }

    // the models
    let lists = engine.models.as_ref().is_some_and(|m| m.list.is_some());
    let needs_model = engine.doc.provides.needs_model;
    let entries = quickai_core::models::recommended(registry, engine);
    let mut pulls: Vec<String> = Vec::new();
    let mut model_file = None;
    let mut known: BTreeSet<String> = BTreeSet::new();
    if lists {
        for pick in &ctx.configuration.models {
            let name = match &pick.source {
                ModelSource::Named { name, .. } => name.clone(),
                ModelSource::Catalog { id, .. } => {
                    let key = format!("catalog:{id}");
                    entries
                        .iter()
                        .filter(|e| e.key == key)
                        .flat_map(|e| e.offers.iter())
                        .find_map(|o| match &o.fetch {
                            Fetch::Reference(name) => Some(name.clone()),
                            Fetch::Files(_) => None,
                        })
                        .ok_or_else(|| {
                            needs(format!(
                                "the recommended list has no name for `{id}` that {} pulls",
                                engine.doc.engine.name
                            ))
                        })?
                }
                ModelSource::File { name } => {
                    return Err(needs(format!(
                        "{name} is a file, and {} takes the name of a model to pull, not a file",
                        engine.doc.engine.name
                    )))
                }
            };
            known.insert(name.clone());
            if !pulls.contains(&name) {
                pulls.push(name);
            }
        }
    } else {
        for (index, wanted) in wanted_downloads(ctx, engine, &models_dir, &entries)?
            .into_iter()
            .enumerate()
        {
            known.insert(wanted.weights.clone());
            if index == 0 && needs_model {
                model_file = Some(wanted.weights.clone());
            }
            if let Some(download) = wanted.download {
                let located = locate(
                    ctx,
                    engine,
                    download,
                    &models_dir,
                    &mut known,
                    wanted.strict,
                )?;
                // a file from outside names a place: that is for the person to read (the plan says it) and to
                // say yes to, as the window's look-up is a button and not a thing a file does; not when the
                // file is to come from somewhere they said, or from a copy they have
                if let (true, true, Some(named), true) = (
                    ctx.outside,
                    !ctx.options.accept_review,
                    wanted.named.as_ref(),
                    located.declared,
                ) {
                    return Err(Failure::new(
                        FailureKind::Asks,
                        format!(
                            "this file says that {} can be had from {named}, which is not a model of the recommended list. If you have read it and mean it, say so with --accept-review (or say where to get {} yourself with --model-source {}=PATH).",
                            wanted.weights, wanted.weights, wanted.weights
                        ),
                    ));
                }
                notes.extend(located.notes.iter().cloned());
                steps.extend(fetch_step(located));
            }
        }
    }
    // a place the person named for a file the configuration does not fetch is a mistake in what they said
    let unused: Vec<&String> = ctx
        .environment
        .models
        .keys()
        .filter(|name| !known.contains(*name))
        .collect();
    if !unused.is_empty() {
        let names: Vec<&str> = known.iter().map(String::as_str).collect();
        return Err(mistake(format!(
            "you said where to get {}, and this configuration fetches no file by that name{}",
            unused
                .iter()
                .map(|n| format!("`{n}`"))
                .collect::<Vec<_>>()
                .join(", "),
            if names.is_empty() {
                " (it fetches none)".to_string()
            } else {
                format!(" (it fetches: {})", names.join(", "))
            }
        )));
    }

    // what is done once they run
    let server = &engine.doc.engine.name;
    let port = values.get(&key("port")).and_then(Value::as_i64).or(engine
        .doc
        .provides
        .default_port
        .map(i64::from));
    later.push(info(
        "start",
        format!(
            "start {server}{}",
            port.map_or(String::new(), |p| format!(" on 127.0.0.1:{p}"))
        ),
        None,
    ));
    for (n, name) in pulls.iter().enumerate() {
        later.push(info(
            "pull",
            format!("pull {name} unless {server} has it"),
            None,
        ));
        if n == 0 {
            later.push(info("load", format!("load {name}"), None));
        }
    }
    if let Some(companion) = &companion {
        if let Some(member) = registry.engine(&companion.id) {
            let cport = values
                .get(&format!("{}.port", member.id))
                .and_then(Value::as_i64)
                .or(member.doc.provides.default_port.map(i64::from));
            later.push(info(
                "companion",
                format!(
                    "start {}{} beside it",
                    member.doc.engine.name,
                    cport.map_or(String::new(), |p| format!(" on 127.0.0.1:{p}"))
                ),
                None,
            ));
        }
    }

    Ok(Plan {
        steps,
        intent: Intent {
            engine: engine.id.clone(),
            values,
            binary,
            companion,
            model_file,
            pulls,
            models_dir,
        },
        later,
        notes,
    })
}
