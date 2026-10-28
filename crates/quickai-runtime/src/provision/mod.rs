//! Getting a configuration running without a window (docs/headless.md).
//!
//! A configuration says what QuickAI runs. The window *selects* it and fetches what is missing when a
//! person presses a button (decision C103: a configuration selects and does not install). The headless
//! program is a person typing its name: running it is the click. So this does, in one call, what the buttons
//! do one by one:
//!
//! 1. read the configuration and review it for this computer, as the window does (`profile::check`): a note
//!    that stops it stops it, and what asks (free text, wider access) is taken only when the person says so;
//!    put over it what the person says about this computer (an environment file, `--set`, `--env`, the
//!    logins), and stop, before anything is fetched, if the configuration needs something nobody gave;
//! 2. work out what is missing, say it as a plan, and stop there if only the plan was asked for;
//! 3. install the programs that are missing and fetch the files of the models, with progress; a file the
//!    person has is used where it is;
//! 4. start the engine (and the program that goes with it, when the configuration has it start), pull and
//!    load a model an engine takes by name, say where everything is, and stay until it is told to stop.
//!
//! The call blocks, reports through a closure and watches a flag: a program with no window needs nothing
//! more. Nothing here names an engine; the documents say how each is installed, started and asked for its
//! models, and `resolve` is the one place that turns a selection into the plan of a run.

mod build;
mod plan;
mod prerequisites;
mod report;
mod run;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use quickai_core::diag::Severity;
use quickai_core::profile::check::{human_bytes, review};
use quickai_core::profile::environment::{template, Environment, Where};
use quickai_core::profile::state::apply;
use quickai_core::profile::{Need, NeedKind, Token};
use quickai_core::value::Value;

pub use build::pick_build;
pub use quickai_core::profile::environment::Secret;
pub use report::{
    in_the_words_of_the_command_line, EndpointInfo, EnvironmentItem, Event, Failure, FailureKind,
    FileInfo, Lack, Outcome, PortUse, PrerequisiteInfo, StepInfo,
};

use crate::config::{build_registry, ConfigDirs};
use crate::configuration_cli;
use crate::gpu::{self, GpuVendor};
use crate::launch::{self, MachineFacts};
use crate::settings::Settings;

/// What the person said besides which configuration.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Options {
    /// Say what would be done and change nothing: no install, no download, no start.
    pub dry_run: bool,
    /// Install and fetch what is needed, and start nothing.
    pub prepare_only: bool,
    /// Take what asks (free text, wider access) in a file from outside.
    pub accept_review: bool,
    /// The build to install when none is installed, instead of the one the configuration or the document
    /// names.
    pub build: Option<String>,
    /// The port the engine listens on, instead of the configuration's.
    pub port: Option<u16>,
    /// The folder the model files are kept in, instead of the engine's own.
    pub models_dir: Option<PathBuf>,
    /// The Python an install that builds an environment uses.
    pub python: Option<PathBuf>,
    /// The cards this computer has, when they cannot be told or are told wrongly. `Some(vec![])` is none.
    pub gpus: Option<Vec<GpuVendor>>,
    /// The hashes (or their first twelve characters) of documents of the person's own that may start a
    /// program QuickAI did not install.
    pub confirmed: Vec<String>,
    /// The build of a program that goes with the engine to install (`--build openwebui=0.11.4`), by its id.
    pub companion_builds: BTreeMap<String, String>,
    /// Files that say what belongs to this computer (`--environment`), in the order given: a later one wins.
    pub environment_files: Vec<PathBuf>,
    /// `<engine>.<field>=<value>`, as `--set` has them.
    pub sets: Vec<String>,
    /// `[program:]NAME=VALUE`, as `--env` has them.
    pub env_lines: Vec<String>,
    /// `FILE=WHERE`, as `--model-source` has them.
    pub model_sources: Vec<String>,
    /// Logins and the address of a mirror, from flags and from the variables a program reads; they win over a file.
    pub fetch: Vec<FetchWord>,
    /// Fetch nothing: use what is here, and say what is missing and where it goes.
    pub offline: bool,
    /// Folders to look in for the files of the models before fetching them.
    pub models_from: Vec<PathBuf>,
    /// Install the packages that bring what the computer lacks (`--install-system-packages`), as the
    /// administrator or with `sudo` and no password. Without it a missing prerequisite is told with the
    /// command that brings it, and nothing is changed.
    pub install_system_packages: bool,
    /// Do not look at what the configuration says the computer has to have, and run no test of the
    /// installed environment (`--skip-prerequisites`).
    pub skip_prerequisites: bool,
    /// The computer to look at for that, when it is described and not this one: where its programs are, the
    /// files its driver keeps, who runs. For a test; `None` is this computer.
    pub machine: Option<crate::host::Machine>,
}

/// One thing said about how files are fetched: a login or an address, and where it was said.
#[derive(Clone, Debug, PartialEq)]
pub struct FetchWord {
    /// `huggingface_token`, `huggingface_endpoint` or `github_token`.
    pub key: String,
    pub value: Secret,
    /// `--hf-token`, `HF_TOKEN`...
    pub origin: String,
}

/// What to do: which configuration, where the application's files are, and how.
#[derive(Clone, Debug)]
pub struct Request {
    pub dirs: ConfigDirs,
    /// The id of a configuration the person keeps, or the path of a file.
    pub target: String,
    pub options: Options,
}

/// The notes of the review that say something is not here, which the plan puts right or says it cannot.
fn the_plan_covers(note: &str) -> bool {
    matches!(
        note,
        "model_to_get" | "model_unseen" | "build_missing" | "build_none" | "companion_missing"
    )
}

fn failed(kind: FailureKind, message: impl Into<String>) -> Outcome {
    Outcome::Failed(Failure::new(kind, message))
}

/// Does what `request` says. Every thing worth saying is passed to `sink` as it happens; `stop` is set by
/// whoever wants it to end (a signal), and the call returns when it has. The last event is always
/// `Finished`.
pub fn up(request: &Request, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
    let outcome = up_inner(request, sink, stop);
    sink(Event::Finished {
        outcome: outcome.word().to_string(),
        message: match &outcome {
            Outcome::Failed(failure) => Some(failure.message.clone()),
            _ => None,
        },
        lack: match &outcome {
            Outcome::Failed(failure) => failure.lack.clone(),
            _ => None,
        },
    });
    outcome
}

/// Makes a relative path of a word into an absolute one, against `base`.
fn absolute(place: Where, base: &Path) -> Where {
    match place {
        Where::Path(path) if Path::new(&path).is_relative() => {
            Where::Path(base.join(path).display().to_string())
        }
        other => other,
    }
}

/// What the person said about this computer, from every place it can be said, put together: the files
/// first, in order, then the words of the command line, then the logins; a later word wins over an earlier.
/// Everything wrong in what was said is told at once, as a mistake.
fn environment_of(
    options: &Options,
    registry: &quickai_core::registry::Registry,
    engine_id: &str,
) -> Result<(Environment, Vec<String>), Failure> {
    let engines = registry.engine_map();
    let mut environment = Environment::default();
    let mut warnings = Vec::new();
    let mut problems: Vec<String> = Vec::new();
    let here = std::env::current_dir().unwrap_or_default();

    for path in &options.environment_files {
        let origin = format!("environment file {}", path.display());
        let text = match std::fs::read_to_string(path) {
            Ok(text) => text,
            Err(e) => {
                problems.push(format!("{origin}: could not be read: {e}"));
                continue;
            }
        };
        match Environment::read(&text, &origin, engines, engine_id) {
            Ok((mut file, unknown)) => {
                warnings.extend(unknown);
                let base = path
                    .parent()
                    .map(|p| {
                        if p.as_os_str().is_empty() {
                            here.clone()
                        } else {
                            std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf())
                        }
                    })
                    .unwrap_or_else(|| here.clone());
                file.models = std::mem::take(&mut file.models)
                    .into_iter()
                    .map(|(name, place)| (name, absolute(place, &base)))
                    .collect();
                #[cfg(unix)]
                {
                    use std::os::unix::fs::PermissionsExt;
                    let holds_secret = file.fetch.huggingface_token.is_some()
                        || file.fetch.github_token.is_some()
                        || file
                            .env
                            .values()
                            .flatten()
                            .any(|l| quickai_core::plan::is_secret_name(&l.name));
                    if holds_secret
                        && std::fs::metadata(path)
                            .is_ok_and(|m| m.permissions().mode() & 0o044 != 0)
                    {
                        warnings.push(format!(
                            "{origin} holds a login or a secret and other people on this computer can read it (chmod 600 it)"
                        ));
                    }
                }
                environment.merge(file);
            }
            Err(found) => problems.extend(found.iter().map(ToString::to_string)),
        }
    }
    for word in &options.sets {
        match word.split_once('=') {
            Some((key, value)) => {
                if let Err(why) =
                    environment.set_text(engines, engine_id, key.trim(), value, "--set")
                {
                    problems.push(format!("--set {word}: {why}"));
                }
            }
            None => problems.push(format!("--set {word}: it is `<engine>.<field>=<value>`")),
        }
    }
    for word in &options.env_lines {
        if let Err(why) = environment.add_env_line(engines, engine_id, word, "--env") {
            problems.push(format!("--env {word}: {why}"));
        }
    }
    for word in &options.model_sources {
        match environment.add_model_source(word, "--model-source") {
            Ok(()) => {
                let file = word.split_once('=').map(|(f, _)| f).unwrap_or_default();
                if let Some(place) = environment.models.remove(file) {
                    environment
                        .models
                        .insert(file.to_string(), absolute(place, &here));
                }
            }
            Err(why) => problems.push(format!("--model-source {word}: {why}")),
        }
    }
    for word in &options.fetch {
        if let Err(why) = environment.set_fetch(&word.key, word.value.expose(), &word.origin) {
            problems.push(format!("{}: {why}", word.origin));
        }
    }
    if problems.is_empty() {
        Ok((environment, warnings))
    } else {
        Err(Failure::new(FailureKind::Mistake, problems.join("\n")))
    }
}

/// How a thing the configuration needs is given, in the words of this program.
fn how_to_give(need: &Need) -> String {
    match &need.kind {
        NeedKind::Token(Token::Huggingface) => {
            "--hf-token, the variable HF_TOKEN, or `huggingface_token` under [fetch] of an environment file".to_string()
        }
        NeedKind::Token(Token::Github) => {
            "--github-token, the variable GITHUB_TOKEN, or `github_token` under [fetch] of an environment file".to_string()
        }
        NeedKind::Setting { key } => {
            let (owner, field) = key.split_once('.').unwrap_or((key, ""));
            format!("--set {key}=VALUE, or `{field}` under [values.{owner}] of an environment file")
        }
        NeedKind::Env { program, name } => format!(
            "--env {program}:{name}=VALUE, or `{name}` under [env.{program}] of an environment file"
        ),
        NeedKind::System { id } => {
            format!("nobody gives {id}: it is looked for on the computer")
        }
    }
}

/// Where `init` writes the file to fill in when it is not told: next to the configuration if that is a file
/// (`vision.toml` gives `vision.environment.toml`), else in the folder it is run in (`<id>.environment.toml`).
pub fn default_environment_path(target: &str) -> PathBuf {
    let path = Path::new(target);
    let is_file = path.extension().is_some_and(|e| e == "toml") || target.contains('/');
    if is_file {
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| "quickai".to_string());
        path.with_file_name(format!("{stem}.environment.toml"))
    } else {
        PathBuf::from(format!("{target}.environment.toml"))
    }
}

/// The sentence that says what the configuration needs and nobody has given.
fn needs_message(unmet: &[&Need], target: &str) -> String {
    let mut lines =
        vec!["this configuration needs something from you before it can run:".to_string()];
    for need in unmet {
        lines.push(format!("  - {}", need.label()));
        if let Some(why) = &need.why {
            lines.push(format!("      {why}"));
        }
        lines.push(format!("      give it with {}", how_to_give(need)));
    }
    let file = default_environment_path(target);
    lines.push(format!(
        "`quickai-run init {target}` writes the file to fill in ({}); give it back with `--environment {}`.",
        file.display(),
        file.display()
    ));
    lines.join("\n")
}

/// The file to fill in for a configuration: what it needs, what belongs to a computer, and the files it
/// fetches, each with where from. Reads the documents and the configuration and nothing else: no network,
/// no disk of the models, no program.
pub fn init(request: &Request) -> Result<String, Failure> {
    let (dirs, options) = (&request.dirs, &request.options);
    let registry = build_registry(dirs);
    let (configuration, _notes, _outside) =
        match configuration_cli::resolve(&registry, &request.target) {
            Ok(found) => found,
            Err(message) => return Err(Failure::new(FailureKind::Unreadable, message)),
        };
    let applied = apply(&registry, &configuration)
        .map_err(|note| Failure::new(FailureKind::Stopped, note.text(registry.vocabulary())))?;
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &[]);
    let facts = launch::host_facts(Path::new(""), machine, None);
    let environment = Environment::default();
    let ctx = plan::Ctx {
        registry: &registry,
        dirs,
        options,
        configuration: &configuration,
        applied: &applied,
        facts: &facts,
        vendors: &[],
        outside: false,
        environment: &environment,
    };
    let files = plan::file_hints(&ctx)?;
    Ok(template(registry.engine_map(), &configuration, &files))
}

fn up_inner(request: &Request, sink: &mut dyn FnMut(Event), stop: &AtomicBool) -> Outcome {
    let (dirs, options) = (&request.dirs, &request.options);
    let registry = build_registry(dirs);
    for d in registry
        .diagnostics
        .iter()
        .filter(|d| d.severity != Severity::Info)
    {
        sink(Event::Say {
            text: format!("config: {d}"),
        });
    }
    let (configuration, read_notes, outside) =
        match configuration_cli::resolve(&registry, &request.target) {
            Ok(found) => found,
            Err(message) => return failed(FailureKind::Unreadable, message),
        };

    // what the person says about this computer
    let (environment, environment_warnings) =
        match environment_of(options, &registry, &configuration.engine) {
            Ok(found) => found,
            Err(failure) => return Outcome::Failed(failure),
        };

    // this computer
    let vendors = options.gpus.clone().unwrap_or_else(gpu::detect);
    let ram = crate::hardware::total_ram_bytes();
    let machine = MachineFacts::detect(ram, &vendors);
    sink(Event::Computer {
        system: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        memory_bytes: ram,
        gpus: vendors.iter().map(|v| v.label().to_string()).collect(),
        gpus_from: if options.gpus.is_some() {
            "named".into()
        } else {
            "detected".into()
        },
    });
    for text in environment_warnings {
        sink(Event::Say { text });
    }
    let items: Vec<EnvironmentItem> = environment
        .describe(registry.engine_map())
        .into_iter()
        .map(|d| EnvironmentItem {
            key: d.key,
            value: d.shown,
            from: d.origin,
        })
        .collect();
    if !items.is_empty() {
        sink(Event::Environment { items });
    }

    // the review: what differs on this computer, and what asks
    let computer = configuration_cli::detect_for(&registry, dirs, machine.clone());
    let settings = Settings::default();
    let inventory = computer.inventory(&registry, dirs, Some(&configuration), &settings);
    let report = review(&registry, &configuration, &read_notes, &inventory, outside);
    let words = |note: &quickai_core::profile::notes::Note| note.text(registry.vocabulary());
    // what is missing is what the plan below is about, and says in its own words; the rest differs on this
    // computer and is for the person to read
    for note in report.notes.iter().filter(|n| !the_plan_covers(n.id)) {
        sink(Event::Review {
            level: note.level.label().to_string(),
            text: words(note),
        });
    }
    if let Some(stopper) = report.stop() {
        return failed(FailureKind::Stopped, words(stopper));
    }
    let asks: Vec<String> = report.asks().map(words).collect();
    if !asks.is_empty() && !options.accept_review {
        return failed(
            FailureKind::Asks,
            format!(
                "this file asks before it is used: {} If you have read it and mean it, say so with --accept-review.",
                asks.join(" ")
            ),
        );
    }
    let mut applied = match apply(&registry, &configuration) {
        Ok(applied) => applied,
        Err(note) => return failed(FailureKind::Stopped, words(&note)),
    };
    for note in &applied.notes {
        sink(Event::Review {
            level: note.level.label().to_string(),
            text: words(note),
        });
    }

    // what the person said goes over what the configuration gave, and what the configuration still
    // needs from them is asked for now, before anything is fetched
    if let Err(why) = environment.apply_to(registry.engine_map(), &mut applied.set) {
        return failed(FailureKind::Mistake, why);
    }
    // what was said with a word of its own (the folder of the models, the port, the Python) is given too
    let mut given = applied.set.clone();
    for member in quickai_core::profile::family(registry.engine_map(), &configuration.engine) {
        let has = |field: &str| member.doc.fields.iter().any(|f| f.id == field);
        if let (Some(dir), true) = (&options.models_dir, has("models_dir")) {
            given.insert(
                format!("{}.models_dir", member.id),
                Value::Str(dir.display().to_string()),
            );
        }
        if let (Some(port), true, true) =
            (options.port, has("port"), member.id == configuration.engine)
        {
            given.insert(format!("{}.port", member.id), Value::from(port));
        }
        if let (Some(python), true) = (&options.python, has("python")) {
            given.insert(
                format!("{}.python", member.id),
                Value::Str(python.display().to_string()),
            );
        }
    }
    let unmet = environment.unmet(&configuration, &given);
    if !unmet.is_empty() {
        return failed(FailureKind::Values, needs_message(&unmet, &request.target));
    }

    // what it needs
    let mut facts = launch::host_facts(Path::new(""), machine.clone(), None);
    if let Some(data) = &dirs.data {
        facts.data_dir = data.display().to_string();
    }
    let ctx = plan::Ctx {
        registry: &registry,
        dirs,
        options,
        configuration: &configuration,
        applied: &applied,
        facts: &facts,
        vendors: &vendors,
        outside,
        environment: &environment,
    };
    let planned = match plan::make(&ctx) {
        Ok(planned) => planned,
        Err(failure) => return Outcome::Failed(failure),
    };
    for text in &planned.notes {
        sink(Event::Say { text: text.clone() });
    }
    // what the computer has to have for it: looked at before anything is fetched, said before the plan
    let mut looked = prerequisites::look(&ctx, planned.intent.binary.as_deref(), sink);
    if let Some(looked) = &looked {
        sink(Event::Prerequisites {
            items: looked.infos(),
        });
    }
    let mut steps: Vec<StepInfo> = planned.steps.iter().map(|s| s.info.clone()).collect();
    if !options.prepare_only {
        steps.extend(planned.later.iter().cloned());
    }
    sink(Event::Plan { steps });
    // and what is missing is installed, when it may be, or stops the run here with the command that brings it
    if let Some(looked) = looked.as_mut() {
        if let Err(halt) = prerequisites::settle(looked, options, options.dry_run, sink, stop) {
            return halt.into_outcome();
        }
    }
    // offline, nothing is fetched: what is not here is said, with where each file goes
    if options.offline {
        let missing: Vec<&FileInfo> = planned
            .steps
            .iter()
            .flat_map(|s| s.info.files.iter())
            .filter(|f| f.how == "fetch")
            .collect();
        if !missing.is_empty() {
            let mut lines = vec![
                "--offline says nothing is fetched, and these files are not here. Put each at the place named (any way you like: scp, rsync, a mounted disk), or name where it is with --model-source or --models-from:".to_string(),
            ];
            for file in missing {
                lines.push(format!(
                    "  {}{}  ->  {}\n      from {}",
                    file.name,
                    file.bytes
                        .map_or(String::new(), |b| format!(" ({})", human_bytes(b))),
                    file.to,
                    file.from
                ));
            }
            return failed(FailureKind::Needs, lines.join("\n"));
        }
    }
    // room for the files, before the first byte of them is fetched (and said by a check)
    if let Err(failure) = check_room(&planned, &planned.intent.models_dir) {
        return Outcome::Failed(failure);
    }
    if options.dry_run {
        // the ports it would listen on, and whether another program has one now
        let ports = ports_of(&registry, &planned.intent);
        if !ports.is_empty() {
            sink(Event::Ports { items: ports });
        }
        return Outcome::Planned;
    }

    // do what is missing
    let plan::Plan {
        steps: todo,
        mut intent,
        later,
        notes: _,
    } = planned;
    let of = todo.len();
    let world = run::World {
        registry: &registry,
        dirs,
        options,
        machine,
        facts: &facts,
    };
    // The environment that was installed is tried before anything is started on it, and before the files of the
    // models are fetched, which are tens of gigabytes: a computer that fails the test is told so after the
    // install and not after the download.
    let tests = looked.as_ref().map(|l| l.scripts()).unwrap_or_default();
    let mut tried = tests.is_empty();
    for (index, step) in todo.iter().enumerate() {
        if stop.load(Ordering::SeqCst) {
            return Outcome::Interrupted;
        }
        if !tried && matches!(step.work, plan::Work::Fetch { .. }) {
            tried = true;
            if let Err(halt) = prerequisites::self_tests(&world, &intent, &tests, sink, stop) {
                return halt.into_outcome();
            }
        }
        match run::do_step(step, index + 1, of, &mut intent, sink, stop) {
            Ok(()) => {}
            Err(run::Halt::Interrupted) => return Outcome::Interrupted,
            Err(run::Halt::Failed(failure)) => return Outcome::Failed(failure),
        }
    }
    if !tried {
        if let Err(halt) = prerequisites::self_tests(&world, &intent, &tests, sink, stop) {
            return halt.into_outcome();
        }
    }
    if options.prepare_only {
        return Outcome::Prepared;
    }

    // run it
    let planned = plan::Plan {
        steps: Vec::new(),
        intent,
        later,
        notes: Vec::new(),
    };
    run::serve(&world, &planned, sink, stop)
}

/// The ports the run of `intent` would listen on (each `<program>.port` of its values), whether another program
/// listens on each now, and, for the engine's, whether what answers there answers as the engine does.
fn ports_of(registry: &quickai_core::registry::Registry, intent: &plan::Intent) -> Vec<PortUse> {
    let mut uses = Vec::new();
    for (key, value) in &intent.values {
        let Some(program) = key.strip_suffix(".port") else {
            continue;
        };
        let Some(port) = (match value {
            Value::Int(n) => u16::try_from(*n).ok(),
            Value::Str(s) => s.trim().parse().ok(),
            _ => None,
        }) else {
            continue;
        };
        let document = registry.engine(program);
        let name = document.map_or_else(|| program.to_string(), |e| e.doc.engine.name.clone());
        let taken = std::net::TcpListener::bind(("127.0.0.1", port)).is_err();
        let answers_as = document
            .filter(|_| taken)
            .and_then(|e| e.doc.launch.attach.as_ref())
            .and_then(|attach| {
                crate::remote::identity(port, &attach.identity.path, &attach.identity.pointer)
            })
            .map(|version| format!("{name} {version}"));
        uses.push(PortUse {
            port,
            program: name,
            taken,
            answers_as,
        });
    }
    uses
}

/// Whether the folder the files go in has room for what the plan fetches (the installs check their own).
fn check_room(planned: &plan::Plan, dir: &Path) -> Result<(), Failure> {
    let bytes: u64 = planned
        .steps
        .iter()
        .filter(|s| matches!(s.work, plan::Work::Fetch { .. }))
        .filter_map(|s| s.info.bytes)
        .sum();
    if bytes == 0 {
        return Ok(());
    }
    // the nearest folder that exists is on the disk the files will be written to
    let mut probe = Some(dir);
    while let Some(path) = probe {
        if path.exists() {
            break;
        }
        probe = path.parent();
    }
    let Some(free) = probe.and_then(crate::prims::install::system_free_space) else {
        return Ok(());
    };
    if free < bytes + bytes / 20 {
        return Err(Failure::new(
            FailureKind::Needs,
            format!(
                "the models need {} and {} has {} free",
                human_bytes(bytes),
                dir.display(),
                human_bytes(free)
            ),
        )
        .with_lack(Lack::Room {
            bytes,
            dir: dir.display().to_string(),
            free,
        }));
    }
    Ok(())
}
