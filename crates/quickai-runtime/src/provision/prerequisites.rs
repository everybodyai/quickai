//! What the computer has to have for the configuration (docs/headless.md, section 12).
//!
//! A configuration says which of its engine's prerequisites it needs (`[[needs]] system = "..."`); the
//! engine's document says how each is looked at and which packages bring it. This looks, before anything is
//! fetched, says what it found, and when something is missing either installs the packages that bring it (only
//! when the person said so) or stops with the exact command; and after the install, before the engine is
//! started, it runs the tests of the installed environment the document has.
//!
//! Nothing here names an engine, a program or a package: the documents do.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use quickai_core::doc::engine::{Fix, PackageManager, PrerequisiteCheck};
use quickai_core::plan::Start;
use quickai_core::prerequisite;
use quickai_core::profile::Configuration;
use quickai_core::registry::Registry;

use super::plan::{Ctx, Intent};
use super::report::{Event, Failure, FailureKind, PrerequisiteInfo};
use super::run::{Halt, Say, World};
use super::Options;
use crate::host::{self, Machine, PythonFacts, ScriptEnd, State};
use crate::prims::install::{self, InstallKind};
use crate::resolve::{self, Wanted};
use crate::runtime::process::child_env;

/// One prerequisite the configuration needs, as the documents and the configuration say it.
#[derive(Clone, Debug)]
pub(super) struct Item {
    pub id: String,
    pub label: String,
    /// The engine whose document names it.
    pub engine: String,
    /// The configuration's words about why it needs it.
    pub why: Option<String>,
    pub hint: Option<String>,
    pub check: PrerequisiteCheck,
    pub fix: Fix,
    pub timeout: Duration,
}

impl Item {
    fn is_script(&self) -> bool {
        matches!(self.check, PrerequisiteCheck::PythonScript { .. })
    }
}

/// What looking at the computer found for one item.
#[derive(Clone, Debug)]
pub(super) struct Found {
    pub item: Item,
    pub state: State,
    /// The packages that would bring it on this computer: none when none are named for its package manager,
    /// when a variable decided, or when the Python is not the system's.
    pub packages: Vec<String>,
}

impl Found {
    fn missing(&self) -> bool {
        matches!(self.state, State::Missing(_))
    }

    fn info(&self) -> PrerequisiteInfo {
        let (state, detail) = if self.item.is_script() {
            (
                "later",
                "a test of the installed environment, run after the install and before the models are fetched"
                    .to_string(),
            )
        } else {
            match &self.state {
                State::Met(detail) => ("met", detail.clone()),
                State::Missing(detail) => ("missing", detail.clone()),
                State::Unchecked(detail) => ("unchecked", detail.clone()),
            }
        };
        PrerequisiteInfo {
            id: self.item.id.clone(),
            label: self.item.label.clone(),
            state: state.to_string(),
            detail,
            why: self.item.why.clone(),
            fix: None,
            hint: if self.missing() {
                self.item.hint.clone()
            } else {
                None
            },
        }
    }
}

/// What the look at the computer is made of, kept for the second look after an install.
pub(super) struct Looked {
    machine: Machine,
    manager: Option<PackageManager>,
    pub found: Vec<Found>,
    /// The Python of the engine's environment, asked once.
    python: Result<EnvPython, String>,
}

/// What is said of a module or the headers that QuickAI's own Python has.
const OWN_PYTHON_HAS_IT: &str = "QuickAI's own Python has it";

/// The Python the engine's environment is made with, as it was asked.
#[derive(Clone, Debug)]
struct EnvPython {
    /// What the Python said of itself; `None` for QuickAI's own (C181), which is downloaded with the install and has what
    /// an environment needs (the venv module and the headers).
    facts: Option<PythonFacts>,
    /// The environment is made already (this is the one beside an installed launcher): what the Python needed to make
    /// it was there.
    made: bool,
}

impl Looked {
    /// The tests of the installed environment, to run after the install.
    pub fn scripts(&self) -> Vec<Item> {
        self.found
            .iter()
            .filter(|f| f.item.is_script())
            .map(|f| f.item.clone())
            .collect()
    }

    /// What is said about each, with the command that brings what is missing and can be brought.
    pub fn infos(&self) -> Vec<PrerequisiteInfo> {
        let command = self.command();
        self.found
            .iter()
            .map(|f| {
                let mut info = f.info();
                if f.missing() && !f.packages.is_empty() {
                    info.fix = command.clone();
                }
                info
            })
            .collect()
    }

    /// Every package of what is missing, once each, and the command that brings them.
    fn packages(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for found in self.found.iter().filter(|f| f.missing()) {
            for package in &found.packages {
                if !out.contains(package) {
                    out.push(package.clone());
                }
            }
        }
        out
    }

    fn command(&self) -> Option<String> {
        let packages = self.packages();
        let manager = self.manager?;
        if packages.is_empty() {
            return None;
        }
        Some(host::command_line(
            manager,
            &packages,
            self.machine.uid == 0,
        ))
    }

    fn look_again(&mut self) {
        self.found = look_at(
            &self.found_items(),
            &self.machine,
            self.manager,
            &self.python,
        );
    }

    fn found_items(&self) -> Vec<Item> {
        self.found.iter().map(|f| f.item.clone()).collect()
    }
}

/// The items the configuration needs that apply to this computer.
fn items(registry: &Registry, configuration: &Configuration, ctx: &Ctx<'_>) -> Vec<Item> {
    prerequisite::wanted(registry, configuration, ctx.facts)
        .into_iter()
        .map(|w| Item {
            id: w.item.id.clone(),
            label: w.item.label.clone(),
            engine: w.engine.id.clone(),
            why: w.why.map(str::to_string),
            hint: w.item.hint.clone(),
            check: w.item.check.clone(),
            fix: w.item.fix.clone(),
            timeout: w.timeout(),
        })
        .collect()
}

/// The folder of the environment beside a launcher that a `python_app` install wrote.
fn environment_python(launcher: &Path) -> PathBuf {
    let venv = launcher.parent().unwrap_or(Path::new("")).join("venv");
    if cfg!(windows) {
        venv.join("Scripts").join("python.exe")
    } else {
        venv.join("bin").join("python")
    }
}

/// The Python the engine's environment is made with: the environment beside the installed launcher, else the
/// Python the install will use (the one the person named, else QuickAI's own when the document has one, else the first
/// of the document's versions).
fn python_of(
    ctx: &Ctx<'_>,
    launcher: Option<&Path>,
    machine: &Machine,
) -> Result<EnvPython, String> {
    let engine = ctx
        .registry
        .engine(&ctx.configuration.engine)
        .ok_or_else(|| "there is no such engine".to_string())?;
    if let Some(launcher) = launcher {
        let python = environment_python(launcher);
        return if python.is_file() {
            host::python_facts(&python, &[]).map(|facts| EnvPython {
                facts: Some(facts),
                made: true,
            })
        } else {
            Err(format!(
                "there is no Python environment beside {}, so it is not known which Python it uses",
                launcher.display()
            ))
        };
    }
    let Some(spec) = install::InstallSpec::from_engine(engine, ctx.facts) else {
        return Err("this engine has no install that makes a Python environment".to_string());
    };
    let InstallKind::PythonApp(app) = &spec.kind else {
        return Err("this engine's install does not make a Python environment".to_string());
    };
    // QuickAI's own Python, when the person names none: nothing of the computer's is used
    if ctx.options.python.is_none() && app.own.is_some() {
        return Ok(EnvPython {
            facts: None,
            made: false,
        });
    }
    // the places a Python is looked for are the computer's: this one's, or the one that is described
    let at = if ctx.options.machine.is_some() {
        install::Where {
            path_var: machine.path_variable(),
            home: None,
        }
    } else {
        install::Where::this_machine()
    };
    let found =
        install::find_python(&app.versions, ctx.options.python.as_deref(), &at).map_err(|why| {
            format!(
                "{} (the install will say the same)",
                super::report::in_the_words_of_the_command_line(&why)
            )
        })?;
    host::python_facts(&found.program, &found.args).map(|facts| EnvPython {
        facts: Some(facts),
        made: false,
    })
}

/// Looks at the computer for each item. A test of the installed environment is not run here: it waits for the
/// install.
fn look_at(
    items: &[Item],
    machine: &Machine,
    manager: Option<PackageManager>,
    python: &Result<EnvPython, String>,
) -> Vec<Found> {
    items
        .iter()
        .map(|item| {
            let brings = |version: Option<(u32, u32)>| -> Vec<String> {
                manager
                    .and_then(|m| prerequisite::packages(&item.fix, m, version))
                    .unwrap_or_default()
            };
            let (state, packages) = match &item.check {
                PrerequisiteCheck::Program { any_of, env } => {
                    let (state, by_variable) = host::check_program(machine, any_of, env.as_deref());
                    let packages = if matches!(state, State::Missing(_)) && !by_variable {
                        brings(
                            python
                                .as_ref()
                                .ok()
                                .and_then(|p| p.facts.as_ref())
                                .map(|f| f.version),
                        )
                    } else {
                        Vec::new()
                    };
                    (state, packages)
                }
                PrerequisiteCheck::PythonModule { module } => match python {
                    // an environment that is made was made with what the module is for
                    Ok(python) if python.made => (
                        State::Met("the environment is made".to_string()),
                        Vec::new(),
                    ),
                    Ok(EnvPython { facts: None, .. }) => {
                        (State::Met(OWN_PYTHON_HAS_IT.to_string()), Vec::new())
                    }
                    Ok(EnvPython {
                        facts: Some(facts), ..
                    }) => {
                        let state = host::check_python_module(facts, module);
                        let packages =
                            if matches!(state, State::Missing(_)) && facts.is_the_systems() {
                                brings(Some(facts.version))
                            } else {
                                Vec::new()
                            };
                        (state, packages)
                    }
                    Err(why) => (State::Unchecked(why.clone()), Vec::new()),
                },
                PrerequisiteCheck::PythonHeader { file } => match python {
                    Ok(EnvPython { facts: None, .. }) => {
                        (State::Met(OWN_PYTHON_HAS_IT.to_string()), Vec::new())
                    }
                    Ok(EnvPython {
                        facts: Some(facts), ..
                    }) => {
                        let state = host::check_python_header(facts, file);
                        // the package is for the Python the computer's package manager brings
                        let packages =
                            if matches!(state, State::Missing(_)) && facts.is_the_systems() {
                                brings(Some(facts.version))
                            } else {
                                Vec::new()
                            };
                        (state, packages)
                    }
                    Err(why) => (State::Unchecked(why.clone()), Vec::new()),
                },
                PrerequisiteCheck::NvidiaDriver { at_least } => {
                    (host::check_nvidia_driver(machine, at_least), Vec::new())
                }
                PrerequisiteCheck::PythonScript { .. } => (
                    State::Unchecked("run after the install".to_string()),
                    Vec::new(),
                ),
            };
            Found {
                item: item.clone(),
                state,
                packages,
            }
        })
        .collect()
}

/// Looks at the computer for what the configuration needs. `None` when it needs nothing of the computer, or
/// the person said not to look.
pub(super) fn look(ctx: &Ctx<'_>, launcher: Option<&Path>, say: Say<'_>) -> Option<Looked> {
    let wanted = items(ctx.registry, ctx.configuration, ctx);
    if wanted.is_empty() {
        return None;
    }
    if ctx.options.skip_prerequisites {
        say(Event::Say {
            text: format!(
                "Not looking at what this computer has to have ({}), as you said.",
                wanted
                    .iter()
                    .map(|i| i.label.as_str())
                    .collect::<Vec<_>>()
                    .join("; ")
            ),
        });
        return None;
    }
    let machine = ctx.options.machine.clone().unwrap_or_else(Machine::this);
    let manager = host::package_manager(&machine);
    let needs_python = wanted.iter().any(|i| {
        matches!(
            i.check,
            PrerequisiteCheck::PythonHeader { .. }
                | PrerequisiteCheck::PythonModule { .. }
                | PrerequisiteCheck::PythonScript { .. }
        ) || PackageManager::ALL
            .iter()
            .flat_map(|m| i.fix.packages(*m))
            .any(|p| p.contains("${python."))
    });
    let python = if needs_python {
        python_of(ctx, launcher, &machine)
    } else {
        Err("not asked".to_string())
    };
    let found = look_at(&wanted, &machine, manager, &python);
    Some(Looked {
        machine,
        manager,
        found,
        python,
    })
}

/// The look is over: what is missing is either installed, when the person said it may be and it can be, or told
/// with the command that brings it. Nothing is changed by `check` (`dry_run`).
pub(super) fn settle(
    looked: &mut Looked,
    options: &Options,
    dry_run: bool,
    say: Say<'_>,
    stop: &AtomicBool,
) -> Result<(), Halt> {
    let missing = |looked: &Looked| looked.found.iter().any(Found::missing);
    if !missing(looked) {
        return Ok(());
    }
    let packages = looked.packages();
    let mut why_not: Option<String> = None;
    if dry_run {
        why_not = Some(if packages.is_empty() {
            "`check` changes nothing; --skip-prerequisites leaves this look out.".to_string()
        } else {
            "`check` changes nothing; `up` or `prepare` with --install-system-packages installs the packages."
                .to_string()
        });
    } else if !options.install_system_packages {
        why_not = Some(if packages.is_empty() {
            "--skip-prerequisites leaves this look out.".to_string()
        } else {
            "quickai-run can install the packages itself with --install-system-packages (it has to be the administrator, or sudo has to need no password); --skip-prerequisites leaves this look out."
                .to_string()
        });
    } else if options.offline {
        why_not = Some(
            "--offline says nothing is fetched, and a package manager fetches; nothing was installed."
                .to_string(),
        );
    } else if packages.is_empty() {
        why_not = Some(
            "no package of the document brings what is missing on this computer, so there is nothing to install."
                .to_string(),
        );
    }
    if why_not.is_none() {
        let Some(manager) = looked.manager else {
            return Err(failure(
                looked,
                Some("this computer has none of the package managers the document names (apt-get, dnf, zypper, pacman, apk)."),
            )
            .into());
        };
        let elevation = host::elevation(&looked.machine);
        if elevation == host::Elevation::Unavailable {
            return Err(failure(
                looked,
                Some("quickai-run is not the administrator and sudo asks for a password, so it cannot install them: run the command yourself, or run quickai-run as the administrator."),
            )
            .into());
        }
        say(Event::Say {
            text: format!(
                "Installing {} with {}.",
                packages.join(", "),
                manager.program()
            ),
        });
        let machine = looked.machine.clone();
        let installed = host::install_packages(
            &machine,
            manager,
            &packages,
            elevation,
            &mut |line| {
                say(Event::Say {
                    text: format!("  {line}"),
                })
            },
            stop,
        );
        if let Err(why) = installed {
            // a stop is the person's, and not a failure of the packages
            if stop.load(Ordering::SeqCst) {
                return Err(Halt::Interrupted);
            }
            let mut f = failure(looked, None);
            f.message = format!("installing the packages failed: {why}\n{}", f.message);
            return Err(f.into());
        }
        looked.look_again();
        say(Event::Prerequisites {
            items: looked.infos(),
        });
        if !missing(looked) {
            return Ok(());
        }
        return Err(failure(
            looked,
            Some("the packages were installed and what is listed is still missing."),
        )
        .into());
    }
    Err(failure(looked, why_not.as_deref()).into())
}

/// The sentence that says what the computer lacks, why, and the command that brings it.
fn failure(looked: &Looked, closing: Option<&str>) -> Failure {
    let missing: Vec<&Found> = looked.found.iter().filter(|f| f.missing()).collect();
    let mut lines = vec![format!(
        "this computer lacks {} the configuration needs:",
        if missing.len() == 1 {
            "something".to_string()
        } else {
            format!("{} things", missing.len())
        }
    )];
    for found in &missing {
        let State::Missing(detail) = &found.state else {
            continue;
        };
        lines.push(format!("  - {}: {detail}", found.item.label));
        if let Some(why) = &found.item.why {
            lines.push(format!("      {why}"));
        }
        if let Some(hint) = &found.item.hint {
            lines.push(format!("      {hint}"));
        }
    }
    if let Some(command) = looked.command() {
        lines.push("bring what packages can with:".to_string());
        lines.push(format!("  {command}"));
        let unbrought: Vec<&str> = missing
            .iter()
            .filter(|f| f.packages.is_empty())
            .map(|f| f.item.label.as_str())
            .collect();
        if !unbrought.is_empty() {
            lines.push(format!("that does not bring: {}", unbrought.join("; ")));
        }
    }
    if let Some(closing) = closing {
        lines.push(closing.to_string());
    }
    Failure::new(FailureKind::Computer, lines.join("\n"))
}

// ---- the tests of the installed environment -------------------------------------------------------------

/// Runs the tests of the installed environment, one after the other, before the engine is started. A test
/// that cannot be run (the environment's Python is not where the launcher is) is said and left out; one that
/// fails stops the run with what it printed.
pub(super) fn self_tests(
    world: &World<'_>,
    intent: &Intent,
    tests: &[Item],
    say: Say<'_>,
    stop: &AtomicBool,
) -> Result<(), Halt> {
    let engine = world.registry.engine(&intent.engine);
    let Some(engine) = engine else {
        return Ok(());
    };
    fn tell(say: Say<'_>, test: &Item, state: &str, output: Vec<String>) {
        say(Event::SelfTest {
            id: test.id.clone(),
            label: test.label.clone(),
            state: state.to_string(),
            output,
        })
    }
    fn skip(say: Say<'_>, test: &Item, why: String) {
        say(Event::Say {
            text: format!("Not running the test \"{}\": {why}", test.label),
        })
    }
    for test in tests {
        let PrerequisiteCheck::PythonScript { asset, .. } = &test.check else {
            continue;
        };
        if test.engine != intent.engine {
            skip(
                say,
                test,
                "it belongs to a program that goes with the engine".to_string(),
            );
            continue;
        }
        let Some(launcher) = intent.binary.as_deref() else {
            skip(
                say,
                test,
                format!("{} is not installed", engine.doc.engine.name),
            );
            continue;
        };
        let python = environment_python(launcher);
        if !python.is_file() {
            skip(
                say,
                test,
                format!(
                    "there is no Python environment beside {}",
                    launcher.display()
                ),
            );
            continue;
        }
        // the environment the engine is started with: the same plan, so the same variables and folders
        let own: std::collections::BTreeMap<_, _> = intent
            .values
            .iter()
            .filter(|(k, _)| k.starts_with(&format!("{}.", intent.engine)))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let wanted = Wanted {
            engine: intent.engine.clone(),
            model_path: None,
            model_name: None,
            values: own,
            binary: Some(launcher.display().to_string()),
            proxy: false,
            models_dir: Some(intent.models_dir.clone()),
        };
        let mut notes = Vec::new();
        let resolved = resolve::resolve(
            world.registry,
            world.dirs,
            world.machine.clone(),
            &wanted,
            &mut notes,
        )
        .map_err(|why| {
            Halt::Failed(Failure::new(
                FailureKind::Computer,
                format!(
                    "the test \"{}\" could not be prepared: {}",
                    test.label,
                    match why {
                        resolve::ResolveError::Planner(problems) => problems
                            .iter()
                            .map(|d| d.to_string())
                            .collect::<Vec<_>>()
                            .join("\n"),
                        other => format!("{other:?}"),
                    }
                ),
            ))
        })?;
        let spawn = resolved
            .plan
            .units
            .first()
            .and_then(|unit| {
                unit.start.iter().find_map(|s| match s {
                    Start::Spawn(spawn) => Some(spawn),
                    Start::Attach(_) => None,
                })
            })
            .cloned();
        let Some(spawn) = spawn else {
            skip(
                say,
                test,
                "the engine's document starts nothing to take the environment from".to_string(),
            );
            continue;
        };
        for dir in &spawn.mkdir {
            let _ = std::fs::create_dir_all(dir);
        }
        let env = child_env(&spawn, &|name| std::env::var(name).ok());
        // the script is one of the application's own files, put in a folder that is made for it and removed
        let temp = env
            .set
            .iter()
            .rev()
            .find(|(name, _)| name == "TMPDIR")
            .map(|(_, value)| PathBuf::from(value))
            .unwrap_or_else(std::env::temp_dir);
        let work = temp.join(format!("quickai-selftest-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let file_name = Path::new(asset)
            .file_name()
            .map(|n| n.to_os_string())
            .unwrap_or_else(|| "selftest.py".into());
        let script = work.join(file_name);
        if let Err(e) = std::fs::create_dir_all(&work)
            .and_then(|_| crate::assets::put(asset, &script).map(|_| ()))
        {
            let _ = std::fs::remove_dir_all(&work);
            return Err(Halt::Failed(Failure::new(
                FailureKind::Computer,
                format!(
                    "the test \"{}\" could not be put on the disk: {e}",
                    test.label
                ),
            )));
        }
        say(Event::Say {
            text: format!("Testing the environment: {}...", test.label),
        });
        tell(say, test, "running", Vec::new());
        let end = host::run_script(&python, &script, &env, Some(&work), test.timeout, stop);
        let _ = std::fs::remove_dir_all(&work);
        match end {
            ScriptEnd::Passed(lines) => tell(say, test, "passed", lines),
            ScriptEnd::Stopped => return Err(Halt::Interrupted),
            ScriptEnd::Failed { how, lines } => {
                tell(say, test, "failed", lines.clone());
                let mut text = vec![format!(
                    "the environment of {} failed a test, \"{}\": it {how}.",
                    engine.doc.engine.name, test.label
                )];
                for line in &lines {
                    text.push(format!("  | {line}"));
                }
                if let Some(why) = &test.why {
                    text.push(why.clone());
                }
                if let Some(hint) = &test.hint {
                    text.push(hint.clone());
                }
                text.push(
                    "Nothing was started. Put that right and run the same command again; --skip-prerequisites leaves the test out."
                        .to_string(),
                );
                return Err(Halt::Failed(Failure::new(
                    FailureKind::Computer,
                    text.join("\n"),
                )));
            }
        }
    }
    Ok(())
}

#[cfg(all(test, target_os = "linux"))]
mod tests {
    use super::*;
    use crate::testkit::scratch;
    use std::os::unix::fs::PermissionsExt;

    fn program(path: &Path, body: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    fn item(id: &str, check: PrerequisiteCheck, apt: &[&str]) -> Item {
        Item {
            id: id.into(),
            label: id.into(),
            engine: "comfyui".into(),
            why: None,
            hint: None,
            check,
            fix: Fix {
                apt: apt.iter().map(|p| p.to_string()).collect(),
                ..Fix::default()
            },
            timeout: Duration::from_secs(10),
        }
    }

    /// Y99c: the venv module is asked of the Python that is going to make the environment, and brings its package
    /// (`python3.12-venv`) only when that Python is the system's; one that makes no environment because it is made is not
    /// asked at all; a Python that could not be found leaves the question unasked.
    #[test]
    fn p3_y99c_the_venv_module_is_asked_of_the_python_that_will_make_the_environment_and_of_no_other(
    ) {
        let base = scratch("y99c");
        let bin = base.join("bin");
        let asked = base.join("asked");
        let marker = base.join("no-ensurepip");
        let python = |prefix: &str| {
            program(
                &bin.join("python3.12"),
                &format!(
                    "case \"$3\" in\n*sysconfig*) printf '3 12\\n/x/include\\n{prefix}\\n'; exit 0;;\n*ensurepip*) echo asked >> '{asked}'; if [ -f '{marker}' ]; then echo \"ModuleNotFoundError: No module named 'ensurepip'\" >&2; exit 1; fi; exit 0;;\nesac\nexit 9",
                    asked = asked.display(),
                    marker = marker.display()
                ),
            );
            host::python_facts(&bin.join("python3.12"), &[]).unwrap()
        };
        program(&bin.join("apt-get"), "exit 0");
        let machine = Machine {
            path: vec![bin.clone()],
            root: base.join("root"),
            vars: Default::default(),
            uid: 1000,
        };
        let module = || PrerequisiteCheck::PythonModule {
            module: "ensurepip".into(),
        };
        let wanted = vec![item(
            "python_venv",
            module(),
            &["python${python.version}-venv"],
        )];
        let look = |facts: Result<PythonFacts, String>, made: bool| {
            look_at(
                &wanted,
                &machine,
                Some(PackageManager::Apt),
                &facts.map(|facts| EnvPython {
                    facts: Some(facts),
                    made,
                }),
            )
        };

        // the module is there
        let found = look(Ok(python("/usr")), false);
        assert!(
            matches!(&found[0].state, State::Met(said) if said == "Python 3.12 imports ensurepip"),
            "{found:?}"
        );
        assert!(found[0].packages.is_empty());

        // the system's Python lacks it: the package for that Python brings it
        std::fs::write(&marker, "").unwrap();
        let found = look(Ok(python("/usr")), false);
        assert!(
            matches!(&found[0].state, State::Missing(said) if said.contains("No module named 'ensurepip'")),
            "{found:?}"
        );
        assert_eq!(found[0].packages, ["python3.12-venv"]);

        // a Python of pyenv's or uv's lacks it: the system's package would not put it right
        let found = look(Ok(python("/home/me/.pyenv/versions/3.12.1")), false);
        assert!(matches!(found[0].state, State::Missing(_)));
        assert!(found[0].packages.is_empty(), "{:?}", found[0].packages);

        // an environment that is made was made: nothing is asked of it
        let _ = std::fs::remove_file(&asked);
        let found = look(Ok(python("/usr")), true);
        assert!(matches!(&found[0].state, State::Met(said) if said == "the environment is made"));
        assert!(!asked.exists(), "the Python was not asked");

        // no Python found: the question is not asked, and the answer says why
        let found = look(Err("no Python 3.13 was found".into()), false);
        assert!(
            matches!(&found[0].state, State::Unchecked(said) if said == "no Python 3.13 was found")
        );
        let _ = std::fs::remove_dir_all(&*base);
    }
}
