//! Doing the steps, and then running what they made ready (docs/headless.md, sections 4 and 5).
//!
//! Everything here is the window's own machinery without the window: an install or a download is a `Job`
//! that is polled, a run is a `Session` that is polled, and what they say arrives as events. A person who
//! interrupts (the stop flag) stops an install at once and cleanly, leaves a download where it is, and stops
//! a program that runs with its own grace.

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use quickai_core::guard::{judge, Verdict};
use quickai_core::plan::{Endpoint, HostFacts, Parent, RunPlan, Start};
use quickai_core::registry::Registry;
use quickai_core::value::Value;

use super::plan::{Companion, Intent, Plan, Slot, Step, Work};
use super::report::{
    in_the_words_of_the_command_line, EndpointInfo, Event, Failure, FailureKind, Outcome,
};
use super::Options;
use crate::config::ConfigDirs;
use crate::downloader;
use crate::events::ServerState;
use crate::jobs::Job;
use crate::launch::{self, MachineFacts};
use crate::prims::engine_models::{list_models, spawn_pull, ListSpec, PullSpec};
use crate::prims::install;
use crate::resolve::{self, ResolveError, Wanted};
use crate::runtime::Session;

pub(super) type Say<'a> = &'a mut dyn FnMut(Event);

/// The plan of a run and the sentences said on the way to it, for a model (or none).
type Planner<'a> = dyn Fn(Option<&str>) -> Result<(resolve::Resolved, Vec<String>), Halt> + 'a;

/// How a step or a run was cut short.
pub(super) enum Halt {
    /// The person asked it to stop.
    Interrupted,
    Failed(Failure),
}

impl From<Failure> for Halt {
    fn from(failure: Failure) -> Halt {
        Halt::Failed(failure)
    }
}

impl Halt {
    /// How the run ends when this cut it short.
    pub(super) fn into_outcome(self) -> Outcome {
        match self {
            Halt::Interrupted => Outcome::Interrupted,
            Halt::Failed(failure) => Outcome::Failed(failure),
        }
    }
}

fn pause() {
    std::thread::sleep(Duration::from_millis(100));
}

/// Waits for a job, passing on what it says; stops it when the person asks. `cancel` stops the work itself
/// where the work can be stopped (an install), and `waits` says whether to wait for it to end then.
fn drive<T>(
    mut job: Job<T>,
    index: usize,
    say: Say<'_>,
    stop: &AtomicBool,
    cancel: &dyn Fn(),
    waits: bool,
) -> Result<T, Halt> {
    let mut last_sent = Instant::now() - Duration::from_secs(10);
    let mut last_detail: Option<String> = None;
    let mut stopping_since: Option<Instant> = None;
    loop {
        if let Some(outcome) = job.poll() {
            for line in job.take_statuses() {
                say(Event::Say { text: line });
            }
            return match outcome {
                Ok(value) => Ok(value),
                Err(_) if stopping_since.is_some() => Err(Halt::Interrupted),
                Err(message) => Err(Halt::Failed(Failure::new(
                    FailureKind::Fetch,
                    in_the_words_of_the_command_line(&message),
                ))),
            };
        }
        for line in job.take_statuses() {
            say(Event::Say { text: line });
        }
        let detail = job.detail().map(str::to_string);
        if detail != last_detail || last_sent.elapsed() >= Duration::from_secs(2) {
            last_detail = detail.clone();
            last_sent = Instant::now();
            say(Event::Progress {
                index,
                done: job.downloaded,
                total: job.total,
                detail,
            });
        }
        if stop.load(Ordering::SeqCst) {
            cancel();
            if !waits {
                return Err(Halt::Interrupted);
            }
            let since = *stopping_since.get_or_insert_with(Instant::now);
            if since.elapsed() > Duration::from_secs(60) {
                return Err(Halt::Interrupted);
            }
        }
        pause();
    }
}

/// Does one step of the plan.
pub(super) fn do_step(
    step: &Step,
    index: usize,
    of: usize,
    intent: &mut Intent,
    say: Say<'_>,
    stop: &AtomicBool,
) -> Result<(), Halt> {
    say(Event::Step {
        index,
        of,
        text: step.info.text.clone(),
    });
    match &step.work {
        Work::Install {
            slot,
            spec,
            request,
        } => {
            let cancel = spec.cancel.clone();
            let job = install::spawn_install_build((**spec).clone(), request.clone(), || {});
            let binary = drive(
                job,
                index,
                say,
                stop,
                &|| cancel.store(true, Ordering::SeqCst),
                true,
            )?;
            match slot {
                Slot::Main => intent.binary = Some(binary.clone()),
                Slot::Companion => {
                    if let Some(c) = intent.companion.as_mut() {
                        c.binary = Some(binary.clone());
                    }
                }
            }
        }
        Work::Fetch { download, with } => {
            let job = downloader::spawn_files_with(
                download.clone(),
                intent.models_dir.clone(),
                with.clone(),
                || {},
            );
            drive(job, index, say, stop, &|| {}, false)?;
        }
    }
    say(Event::StepDone { index });
    Ok(())
}

/// What a program that runs has said, and where it is.
struct Running {
    name: String,
    session: Session,
    state: ServerState,
    /// The last lines it wrote, for the sentence that says why it stopped.
    tail: VecDeque<String>,
}

impl Running {
    fn new(name: &str) -> Running {
        Running {
            name: name.to_string(),
            session: Session::new(),
            state: ServerState::Idle,
            tail: VecDeque::new(),
        }
    }

    /// Passes on what the program wrote and the changes of its state.
    fn pump(&mut self, say: Say<'_>) {
        let mut lines = Vec::new();
        self.session.poll(&mut |line| lines.push(line), &mut |_| {});
        for line in lines {
            if self.tail.len() == 12 {
                self.tail.pop_front();
            }
            self.tail.push_back(line.clone());
            say(Event::Log {
                program: self.name.clone(),
                line,
            });
        }
        let now = self.session.state();
        if now != self.state {
            self.state = now;
            say(Event::State {
                program: self.name.clone(),
                state: now.label().to_ascii_lowercase(),
            });
        }
    }

    fn why_it_stopped(&self) -> String {
        if self.tail.is_empty() {
            format!("{} stopped", self.name)
        } else {
            format!(
                "{} stopped; the last it wrote was:\n  {}",
                self.name,
                self.tail.iter().cloned().collect::<Vec<_>>().join("\n  ")
            )
        }
    }
}

/// Waits until `program` is running. `Ok(false)` when it was asked to stop first.
fn until_running(program: &mut Running, say: Say<'_>, stop: &AtomicBool) -> Result<bool, Halt> {
    loop {
        program.pump(say);
        match program.session.state() {
            ServerState::Running => return Ok(true),
            ServerState::Crashed | ServerState::Idle if program.state != ServerState::Idle => {
                return Err(Halt::Failed(Failure::new(
                    FailureKind::Start,
                    program.why_it_stopped(),
                )))
            }
            _ => {}
        }
        if stop.load(Ordering::SeqCst) {
            return Ok(false);
        }
        pause();
    }
}

/// The program a plan would start, if the person has not yet said it may run (a document of their own
/// names a program QuickAI did not install and did not find).
fn foreign(
    registry: &Registry,
    embedded: &Registry,
    facts: &HostFacts,
    engine_id: &str,
    plan: &RunPlan,
    confirmed: &[String],
    data: Option<&str>,
) -> Option<quickai_core::guard::Foreign> {
    let engine = registry.engine(engine_id)?;
    let program = plan.units.first()?.start.iter().find_map(|s| match s {
        Start::Spawn(spawn) => Some(spawn.program.value.clone()),
        Start::Attach(_) => None,
    })?;
    let trusted = |candidate: &str| {
        let Some(embedded_engine) = embedded.engine(engine_id) else {
            return false;
        };
        let found = launch::locate_binary_in(embedded, engine_id, None, data);
        if std::path::Path::new(candidate) == found {
            return true;
        }
        quickai_core::acquire::root_dir(embedded_engine, facts)
            .is_some_and(|root| std::path::Path::new(candidate).starts_with(root))
    };
    match judge(engine, &program, &trusted, confirmed) {
        Verdict::NeedsConfirmation(foreign) => Some(foreign),
        Verdict::Trusted | Verdict::Confirmed => None,
    }
}

fn asks_about(foreign: &quickai_core::guard::Foreign, name: &str) -> Failure {
    Failure::new(
        FailureKind::Asks,
        format!(
            "a document in your own configuration ({}) says that {name} starts {}, which QuickAI did not install and did not find. Run it only if you wrote that document or trust whoever did; say so with --allow-program {}",
            foreign.files.join(", "),
            foreign.program,
            &foreign.hash[..12]
        ),
    )
}

/// What `run` needs besides the intent.
pub(super) struct World<'a> {
    pub registry: &'a Registry,
    pub dirs: &'a ConfigDirs,
    pub options: &'a Options,
    pub machine: MachineFacts,
    pub facts: &'a HostFacts,
}

/// Starts what the intent says, waits until it runs, tells where, and stays until the person says stop or a
/// program stops by itself.
pub(super) fn serve(world: &World<'_>, plan: &Plan, say: Say<'_>, stop: &AtomicBool) -> Outcome {
    match serve_inner(world, &plan.intent, say, stop) {
        Ok(()) => Outcome::Stopped,
        Err(Halt::Interrupted) => Outcome::Interrupted,
        Err(Halt::Failed(failure)) => Outcome::Failed(failure),
    }
}

fn serve_inner(
    world: &World<'_>,
    intent: &Intent,
    say: Say<'_>,
    stop: &AtomicBool,
) -> Result<(), Halt> {
    let registry = world.registry;
    let engine = registry.engine(&intent.engine).ok_or_else(|| {
        Failure::new(
            FailureKind::Unreadable,
            format!("there is no engine called `{}`", intent.engine),
        )
    })?;
    let name = engine.doc.engine.name.clone();
    let lists = engine.models.as_ref().is_some_and(|m| m.list.is_some());
    let data = world.dirs.data.as_ref().map(|d| d.display().to_string());
    let binary = intent.binary.clone().ok_or_else(|| {
        Failure::new(
            FailureKind::Start,
            format!("{name} is not installed, so it cannot be started"),
        )
    })?;
    let own = |prefix: &str| -> std::collections::BTreeMap<String, Value> {
        intent
            .values
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{prefix}.")))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect()
    };
    let embedded = crate::config::build_registry(&ConfigDirs::default());
    let confirmed = world.options.confirmed.clone();

    // the plan of the engine: an engine that pulls its models starts empty, and is given its model after
    let wanted = |model: Option<&str>| Wanted {
        engine: intent.engine.clone(),
        model_path: None,
        model_name: model.map(str::to_string),
        values: own(&intent.engine),
        binary: Some(binary.display().to_string()),
        proxy: false,
        models_dir: Some(intent.models_dir.clone()),
    };
    // the plan of a run, and the sentences that were said on the way to it
    let run_plan = |model: Option<&str>| -> Result<(resolve::Resolved, Vec<String>), Halt> {
        let mut notes = Vec::new();
        let resolved = resolve::resolve(
            registry,
            world.dirs,
            world.machine.clone(),
            &wanted(model),
            &mut notes,
        );
        let failed = |why: String| Halt::Failed(Failure::new(FailureKind::Start, why));
        match resolved {
            Ok(resolved) => Ok((resolved, notes)),
            Err(ResolveError::ModelNotHere { name }) => Err(failed(format!(
                "the model {name} is not in the models folder"
            ))),
            Err(ResolveError::NoModel) => Err(failed(
                "the models folder holds no model to run".to_string(),
            )),
            Err(ResolveError::UnknownEngine(id)) => {
                Err(failed(format!("there is no engine called `{id}`")))
            }
            Err(ResolveError::NoPort(why)) => Err(failed(why)),
            Err(ResolveError::Planner(problems)) => Err(failed(
                problems
                    .iter()
                    .map(|d| d.to_string())
                    .collect::<Vec<_>>()
                    .join("\n"),
            )),
        }
    };

    let first = if lists {
        None
    } else {
        intent.model_file.as_deref()
    };
    let (resolved, notes) = run_plan(first)?;
    for note in notes {
        say(Event::Say { text: note });
    }
    if let Some(found) = foreign(
        registry,
        &embedded,
        world.facts,
        &intent.engine,
        &resolved.plan,
        &confirmed,
        data.as_deref(),
    ) {
        if !confirmed.iter().any(|c| found.hash.starts_with(c.as_str())) {
            return Err(Halt::Failed(asks_about(&found, &name)));
        }
    }
    let display = first.unwrap_or_default().to_string();

    let mut main = Running::new(&name);
    main.session.start(&resolved.plan, &display, || {});
    main.pump(say);
    let public_port = resolved.plan.public_port;
    if !until_running(&mut main, say, stop)? {
        main.session.stop_blocking();
        return Err(Halt::Interrupted);
    }

    // an engine that pulls its models: ask what it has, pull what it lacks, and load the first
    let mut loaded = first.map(str::to_string);
    if lists && !intent.pulls.is_empty() {
        let result = pull_and_load(world, intent, &mut main, public_port, say, stop, &run_plan);
        match result {
            Ok(model) => loaded = model,
            Err(halt) => {
                main.session.stop_blocking();
                return Err(halt);
            }
        }
    }

    // the program that goes with it
    let mut companion: Option<Running> = None;
    if let Some(c) = &intent.companion {
        match start_companion(
            world,
            intent,
            c,
            &binary,
            public_port,
            say,
            stop,
            &confirmed,
            &embedded,
            data.as_deref(),
        ) {
            Ok(Some(running)) => companion = Some(running),
            Ok(None) => {
                main.session.stop_blocking();
                return Err(Halt::Interrupted);
            }
            Err(halt) => {
                main.session.stop_blocking();
                return Err(halt);
            }
        }
    }

    // where they are
    let mut endpoints = Vec::new();
    if let Some(port) = main.session.port().or(public_port) {
        if !engine.doc.provides.apis.is_empty() {
            endpoints.push(EndpointInfo {
                role: "api".into(),
                program: name.clone(),
                url: format!("http://127.0.0.1:{port}"),
            });
        }
        if let Some(page) = &engine.doc.provides.web_ui {
            let path = if page.path == "/" {
                ""
            } else {
                page.path.as_str()
            };
            endpoints.push(EndpointInfo {
                role: "page".into(),
                program: name.clone(),
                url: format!("http://127.0.0.1:{port}{path}"),
            });
        }
    }
    if let (Some(running), Some(c)) = (&companion, &intent.companion) {
        if let Some(member) = registry.engine(&c.id) {
            if let (Some(port), Some(page)) = (running.session.port(), &member.doc.provides.web_ui)
            {
                let path = if page.path == "/" {
                    ""
                } else {
                    page.path.as_str()
                };
                endpoints.push(EndpointInfo {
                    role: "page".into(),
                    program: member.doc.engine.name.clone(),
                    url: format!("http://127.0.0.1:{port}{path}"),
                });
            }
        }
    }
    say(Event::Ready {
        endpoints,
        model: main.session.model_name().map(str::to_string).or(loaded),
    });

    // until the person says stop, or something stops by itself
    let ended = loop {
        main.pump(say);
        if let Some(running) = companion.as_mut() {
            running.pump(say);
        }
        if stop.load(Ordering::SeqCst) {
            break Ok(());
        }
        if matches!(
            main.session.state(),
            ServerState::Crashed | ServerState::Idle
        ) {
            break Err(Failure::new(FailureKind::Crashed, main.why_it_stopped()));
        }
        if let Some(running) = companion.as_ref() {
            if matches!(
                running.session.state(),
                ServerState::Crashed | ServerState::Idle
            ) {
                break Err(Failure::new(FailureKind::Crashed, running.why_it_stopped()));
            }
        }
        pause();
    };
    say(Event::Stopping);
    if let Some(mut running) = companion {
        running.session.stop_blocking();
        running.pump(say);
    }
    main.session.stop_blocking();
    main.pump(say);
    ended.map_err(Halt::Failed)
}

/// Asks the engine what it has, pulls the names it lacks and loads the first. The name that was loaded.
fn pull_and_load(
    world: &World<'_>,
    intent: &Intent,
    main: &mut Running,
    public_port: Option<u16>,
    say: Say<'_>,
    stop: &AtomicBool,
    run_plan: &Planner<'_>,
) -> Result<Option<String>, Halt> {
    let registry = world.registry;
    let engine = registry.engine(&intent.engine).ok_or_else(|| {
        Failure::new(
            FailureKind::Start,
            format!("there is no engine called `{}`", intent.engine),
        )
    })?;
    let endpoint = Endpoint {
        host: "127.0.0.1".to_string(),
        port: main.session.port().or(public_port),
    };
    let list = quickai_core::models::list_call(engine, world.facts)
        .and_then(Result::ok)
        .map(|call| ListSpec::from_call(&call));
    let listed = |list: &Option<ListSpec>| -> Vec<String> {
        list.as_ref()
            .and_then(|spec| list_models(&endpoint, spec).ok())
            .map(|l| l.rows.into_iter().map(|r| r.id).collect())
            .unwrap_or_default()
    };
    let mut have = listed(&list);
    let total = intent.pulls.len();
    for (n, name) in intent.pulls.iter().enumerate() {
        if quickai_core::models::find_listed(have.iter().map(String::as_str), name).is_some() {
            say(Event::Say {
                text: format!("{} already has {name}", main.name),
            });
            continue;
        }
        if world.options.offline {
            return Err(Halt::Failed(Failure::new(
                FailureKind::Needs,
                format!(
                    "{} does not have {name}, and --offline says nothing is fetched. Pull it once with the network, or run without --offline",
                    main.name
                ),
            )));
        }
        let call = quickai_core::models::pull_call(engine, name)
            .and_then(Result::ok)
            .ok_or_else(|| {
                Failure::new(
                    FailureKind::Fetch,
                    format!("{} has no way to pull a model", main.name),
                )
            })?;
        say(Event::Step {
            index: n + 1,
            of: total,
            text: format!("pull {name}"),
        });
        let (job, handle) = spawn_pull(
            endpoint.clone(),
            PullSpec::from_call(&call),
            name.clone(),
            || {},
        );
        let stopper = handle.clone();
        drive(job, n + 1, say, stop, &move || stopper.cancel(), true)?;
        say(Event::StepDone { index: n + 1 });
        have = listed(&list);
    }
    // the first is the one the engine loads
    let Some(first) = intent.pulls.first() else {
        return Ok(None);
    };
    let (plan, notes) = run_plan(Some(first))?;
    for note in notes {
        say(Event::Say { text: note });
    }
    main.session.load(&plan.plan, first, || {});
    loop {
        main.pump(say);
        if main.session.loading().is_none() {
            break;
        }
        if matches!(
            main.session.state(),
            ServerState::Crashed | ServerState::Idle
        ) {
            return Err(Halt::Failed(Failure::new(
                FailureKind::Start,
                main.why_it_stopped(),
            )));
        }
        if stop.load(Ordering::SeqCst) {
            return Err(Halt::Interrupted);
        }
        pause();
    }
    match main.session.model_name() {
        Some(loaded) => Ok(Some(loaded.to_string())),
        None => {
            say(Event::Say {
                text: format!(
                    "{} is running, but it did not load {first}; the lines above say why",
                    main.name
                ),
            });
            Ok(None)
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn start_companion(
    world: &World<'_>,
    intent: &Intent,
    companion: &Companion,
    main_binary: &std::path::Path,
    public_port: Option<u16>,
    say: Say<'_>,
    stop: &AtomicBool,
    confirmed: &[String],
    embedded: &Registry,
    data: Option<&str>,
) -> Result<Option<Running>, Halt> {
    let registry = world.registry;
    let member = registry.engine(&companion.id).ok_or_else(|| {
        Failure::new(
            FailureKind::Start,
            format!("there is no program called `{}`", companion.id),
        )
    })?;
    let name = member.doc.engine.name.clone();
    let binary = companion.binary.clone().ok_or_else(|| {
        Failure::new(
            FailureKind::Start,
            format!("{name} is not installed, so it cannot be started"),
        )
    })?;
    let values: std::collections::BTreeMap<String, Value> = intent
        .values
        .iter()
        .filter(|(k, _)| k.starts_with(&format!("{}.", companion.id)))
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect();
    let selection = launch::selection_from_values(&companion.id, None, &values, false);
    let mut facts = launch::host_facts(&binary, world.machine.clone(), None);
    if let Some(dir) = data {
        facts.data_dir = dir.to_string();
    }
    facts.parent = Some(Parent {
        id: intent.engine.clone(),
        port: public_port.unwrap_or(0),
    });
    let _ = main_binary;
    let plan = quickai_core::plan::plan(registry, &selection, &facts).map_err(|problems| {
        Failure::new(
            FailureKind::Start,
            problems
                .iter()
                .map(|d| format!("[{name}] {d}"))
                .collect::<Vec<_>>()
                .join("\n"),
        )
    })?;
    if let Some(found) = foreign(
        registry,
        embedded,
        &facts,
        &companion.id,
        &plan,
        confirmed,
        data,
    ) {
        if !confirmed.iter().any(|c| found.hash.starts_with(c.as_str())) {
            return Err(Halt::Failed(asks_about(&found, &name)));
        }
    }
    let mut running = Running::new(&name);
    running.session.start(&plan, "", || {});
    running.pump(say);
    if until_running(&mut running, say, stop)? {
        Ok(Some(running))
    } else {
        running.session.stop_blocking();
        Ok(None)
    }
}
