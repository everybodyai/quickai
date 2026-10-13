//! Group Y of the test plan, the runtime's half of WP4.20: what the computer has to have for a configuration
//! (docs/headless.md, section 12). `provision::up` on a configuration of ComfyUI that says what it needs of
//! the computer, against a computer that is described and not this one: its programs are a folder of
//! stand-ins (a compiler, a package manager, `nvidia-smi`, `sudo`), the Python beside the launcher is a script
//! that answers what the program asks of a Python, and the launcher is a file that is never started, because
//! the tests of this group end before the engine would be (`prepare`, `check`, or a stop at a failure).
//!
//! The documents are the shipped ones: what is checked is what ComfyUI's document says.

use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::config::ConfigDirs;
use crate::gpu::GpuVendor;
use crate::host::Machine;
use crate::provision::{self, Event, FailureKind, Options, Outcome, PrerequisiteInfo, Request};
use crate::testkit::{scratch, Scratch};

/// The configuration: what H3 needs of the computer, in the words of the file.
const NEEDS: &str = r#"
[[needs]]
system = "nvidia_driver"
why = "PyTorch for CUDA 13."

[[needs]]
system = "c_compiler"
why = "The int8 weights run through Triton, which builds a helper in C."

[[needs]]
system = "python_headers"

[[needs]]
system = "triton"
why = "Proves what the two above are for."
"#;

/// A computer, described: where its programs are, where its Python keeps its headers, what the package manager
/// does, and what the test of the environment says.
struct Computer {
    scratch: Scratch,
    bin: PathBuf,
    include: PathBuf,
    launcher: PathBuf,
    /// What the package manager was asked, line by line.
    apt_log: PathBuf,
    /// What the stand-in Python was asked to run, with the environment it was given.
    python_log: PathBuf,
    data: PathBuf,
    config: PathBuf,
    uid: u32,
}

fn write_exe(path: &Path, body: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755)).unwrap();
}

/// The Python of the environment beside `launcher` (`<launcher's folder>/venv/bin/python`): a script that answers
/// what the program asks of a Python, and that runs the test of the environment by writing what it was given to
/// `log` and ending as the files `selftest.code` and `selftest.says` of `says_in` say.
fn python_beside(launcher: &Path, include: &Path, log: &Path, says_in: &Path) {
    write_exe(
        &launcher.parent().unwrap().join("venv/bin/python"),
        &format!(
            r#"if [ "$1" = "-I" ]; then printf '3 12\n%s\n/usr\n' '{include}'; exit 0; fi
echo "ran $1 HOME=$HOME TMPDIR=$TMPDIR PYTHONNOUSERSITE=$PYTHONNOUSERSITE" >> '{log}'
[ -f '{says}' ] && cat '{says}' >&2
[ -f '{code}' ] && exit "$(cat '{code}')"
echo "ok: a kernel was built and ran"
exit 0"#,
            include = include.display(),
            log = log.display(),
            says = says_in.join("selftest.says").display(),
            code = says_in.join("selftest.code").display(),
        ),
    );
}

impl Computer {
    /// A computer with no compiler, no headers, the driver of an old series, apt-get, and a Python of the
    /// system's. `uid` 0 is the administrator.
    fn new(name: &str, uid: u32) -> Computer {
        let scratch = scratch(name);
        let root = scratch.to_path_buf();
        let bin = root.join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        for tool in [
            "sh", "mkdir", "touch", "cat", "env", "chmod", "echo", "printf", "true",
        ] {
            for dir in ["/usr/bin", "/bin"] {
                let from = Path::new(dir).join(tool);
                if from.exists() {
                    let _ = std::os::unix::fs::symlink(&from, bin.join(tool));
                    break;
                }
            }
        }
        let include = root.join("usr-include/python3.12");
        let apt_log = root.join("apt.log");
        let python_log = root.join("python.log");
        let launcher = root.join("engine/comfyui");
        write_exe(&launcher, "exit 0");
        // the Python beside the launcher: it is asked for its facts, and it runs the test of the environment
        python_beside(&launcher, &include, &python_log, &root);
        // the package manager brings what is asked of it
        write_exe(
            &bin.join("apt-get"),
            &format!(
                r#"echo "$*" >> '{log}'
echo "apt: $1"
if [ "$1" = "install" ]; then
  case " $* " in *" build-essential "*) printf '#!/bin/sh\nexit 0\n' > '{gcc}'; chmod +x '{gcc}';; esac
  case " $* " in *" python3.12-dev "*) mkdir -p '{include}'; echo '/* Python.h */' > '{include}/Python.h';; esac
fi
exit 0"#,
                log = apt_log.display(),
                gcc = bin.join("gcc").display(),
                include = include.display(),
            ),
        );
        write_exe(&bin.join("nvidia-smi"), "echo 535.54.03");
        Computer {
            data: root.join("data"),
            config: root.join("config"),
            scratch,
            bin,
            include,
            launcher,
            apt_log,
            python_log,
            uid,
        }
    }

    fn machine(&self) -> Machine {
        Machine {
            path: vec![self.bin.clone()],
            root: self.scratch.join("root"),
            vars: Default::default(),
            uid: self.uid,
        }
    }

    /// The driver is of a new series.
    fn new_driver(&self) {
        write_exe(&self.bin.join("nvidia-smi"), "echo 595.58.03");
    }

    fn has_compiler(&self) {
        write_exe(&self.bin.join("gcc"), "exit 0");
    }

    fn has_headers(&self) {
        std::fs::create_dir_all(&self.include).unwrap();
        std::fs::write(self.include.join("Python.h"), "/* Python.h */").unwrap();
    }

    fn apt_log(&self) -> Vec<String> {
        std::fs::read_to_string(&self.apt_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn python_log(&self) -> Vec<String> {
        std::fs::read_to_string(&self.python_log)
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    /// The test of the environment ends like this.
    fn selftest_ends(&self, code: i32, says: &str) {
        std::fs::write(self.scratch.join("selftest.code"), code.to_string()).unwrap();
        std::fs::write(self.scratch.join("selftest.says"), format!("{says}\n")).unwrap();
    }

    fn configuration(&self, body: &str) -> PathBuf {
        let path = self.scratch.join("h3.toml");
        std::fs::write(
            &path,
            format!(
                "schema = 1\nkind = \"profile\"\nid = \"h3\"\nname = \"H3\"\nengine = \"comfyui\"\n{body}"
            ),
        )
        .unwrap();
        path
    }

    fn options(&self) -> Options {
        Options {
            gpus: Some(vec![GpuVendor::Nvidia]),
            sets: vec![format!("comfyui.bin_override={}", self.launcher.display())],
            machine: Some(self.machine()),
            ..Options::default()
        }
    }

    fn up(&self, options: Options) -> (Outcome, Vec<Event>) {
        let mut events = Vec::new();
        let request = Request {
            dirs: ConfigDirs {
                user: Some(self.config.clone()),
                data: Some(self.data.clone()),
                ..ConfigDirs::default()
            },
            target: self.configuration(NEEDS).display().to_string(),
            options,
        };
        let outcome = provision::up(&request, &mut |e| events.push(e), &AtomicBool::new(false));
        (outcome, events)
    }
}

fn prerequisites(events: &[Event]) -> Vec<Vec<PrerequisiteInfo>> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Prerequisites { items } => Some(items.clone()),
            _ => None,
        })
        .collect()
}

fn state_of<'a>(items: &'a [PrerequisiteInfo], id: &str) -> &'a PrerequisiteInfo {
    items
        .iter()
        .find(|i| i.id == id)
        .unwrap_or_else(|| panic!("no {id} in {items:?}"))
}

fn failure(outcome: &Outcome) -> (&FailureKind, &str) {
    match outcome {
        Outcome::Failed(f) => (&f.kind, f.message.as_str()),
        other => panic!("it should have failed: {other:?}"),
    }
}

fn says(events: &[Event]) -> Vec<String> {
    events
        .iter()
        .filter_map(|e| match e {
            Event::Say { text } => Some(text.clone()),
            _ => None,
        })
        .collect()
}

fn index(events: &[Event], found: impl Fn(&Event) -> bool) -> usize {
    events
        .iter()
        .position(found)
        .unwrap_or_else(|| panic!("not in {events:#?}"))
}

/// Y85: `check` looks at the computer before anything is fetched and says what is there and what is not, with
/// the exact command that brings what a package can, and what no package brings; it exits with a code of its
/// own, changes nothing, and runs nothing but the lookups.
#[test]
fn p3_y85_check_says_what_the_computer_lacks_with_the_command_that_brings_it() {
    let c = Computer::new("y85", 1000);
    let (outcome, events) = c.up(Options {
        dry_run: true,
        ..c.options()
    });
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer, "{message}");
    assert_eq!(outcome.exit_code(), 6);
    // said once, before the plan
    let said = prerequisites(&events);
    assert_eq!(said.len(), 1, "{events:#?}");
    let items = &said[0];
    assert_eq!(
        items.iter().map(|i| i.id.as_str()).collect::<Vec<_>>(),
        ["nvidia_driver", "c_compiler", "python_headers", "triton"],
        "in the order of the file"
    );
    assert_eq!(state_of(items, "nvidia_driver").state, "missing");
    assert!(
        state_of(items, "nvidia_driver")
            .detail
            .contains("the driver is 535.54.03, older than 580"),
        "{items:?}"
    );
    assert_eq!(state_of(items, "c_compiler").state, "missing");
    assert_eq!(state_of(items, "python_headers").state, "missing");
    assert!(state_of(items, "python_headers")
        .detail
        .contains("Python.h is not in"));
    assert_eq!(
        state_of(items, "triton").state,
        "later",
        "a test of the installed environment waits for the install"
    );
    assert_eq!(
        state_of(items, "c_compiler").why.as_deref(),
        Some("The int8 weights run through Triton, which builds a helper in C."),
        "the file's own words"
    );
    // the command that brings what a package can, for this computer's manager and for who is running
    let command =
        "sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev".to_string();
    assert_eq!(
        state_of(items, "c_compiler").fix.as_deref(),
        Some(command.as_str())
    );
    assert_eq!(
        state_of(items, "python_headers").fix.as_deref(),
        Some(command.as_str())
    );
    assert_eq!(
        state_of(items, "nvidia_driver").fix,
        None,
        "a driver is not installed here"
    );
    // the sentence says all of it
    assert!(
        message.contains("this computer lacks 3 things the configuration needs"),
        "{message}"
    );
    assert!(
        message.contains("a C compiler: none of gcc, clang is on the PATH"),
        "{message}"
    );
    assert!(
        message.contains("Triton, which builds a helper in C"),
        "{message}"
    );
    assert!(message.contains(&format!("  {command}")), "{message}");
    assert!(
        message.contains("that does not bring: an NVIDIA driver of the 580 series or newer"),
        "{message}"
    );
    assert!(message.contains("`check` changes nothing"), "{message}");
    // and the plan was said as well, so that a person sees both
    index(&events, |e| matches!(e, Event::Plan { .. }));
    assert!(
        index(&events, |e| matches!(e, Event::Prerequisites { .. }))
            < index(&events, |e| matches!(e, Event::Plan { .. }))
    );
    // nothing was changed or run: no package, no test, no folder of the engine
    assert!(c.apt_log().is_empty());
    assert!(c.python_log().is_empty());
    assert!(!c.data.exists());
}

/// Y86: what the computer has is said as it is: a compiler that is there, headers that are there and a driver
/// of a new series are `met`, with what was found; a computer that lacks nothing goes on, and the test of the
/// installed environment is the only thing left to do.
#[test]
fn p3_y86_what_the_computer_has_is_said_as_found_and_a_computer_that_lacks_nothing_goes_on() {
    let c = Computer::new("y86", 1000);
    c.new_driver();
    c.has_compiler();
    c.has_headers();
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let items = &prerequisites(&events)[0];
    assert_eq!(state_of(items, "nvidia_driver").state, "met");
    assert_eq!(state_of(items, "nvidia_driver").detail, "driver 595.58.03");
    assert_eq!(state_of(items, "c_compiler").state, "met");
    assert!(state_of(items, "c_compiler").detail.starts_with("gcc at "));
    assert_eq!(state_of(items, "python_headers").state, "met");
    assert!(state_of(items, "python_headers")
        .detail
        .ends_with("Python.h"));
    assert!(items.iter().all(|i| i.fix.is_none()));
    assert!(
        c.apt_log().is_empty(),
        "nothing was installed that was there"
    );
}

/// Y87: with `--install-system-packages`, as the administrator, what a package brings is installed (the list of
/// packages is refreshed first, each package once), the computer is looked at again and said, and the run goes
/// on to the test of the environment; what no package brings is still told.
#[test]
fn p3_y87_the_packages_are_installed_when_the_person_says_so_and_the_run_goes_on() {
    let c = Computer::new("y87", 0);
    c.new_driver();
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        c.apt_log(),
        [
            "update",
            "install -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev"
        ],
        "the lists first, then the two packages, once each, the install told to wait for a lock another process holds"
    );
    let looks = prerequisites(&events);
    assert_eq!(looks.len(), 2, "before and after the install");
    assert_eq!(state_of(&looks[0], "c_compiler").state, "missing");
    assert_eq!(
        state_of(&looks[0], "c_compiler").fix.as_deref(),
        Some("apt-get update && apt-get install -y build-essential python3.12-dev"),
        "as the administrator no sudo is typed"
    );
    assert_eq!(state_of(&looks[1], "c_compiler").state, "met");
    assert_eq!(state_of(&looks[1], "python_headers").state, "met");
    let said = says(&events);
    assert!(
        said.iter()
            .any(|s| s == "Installing build-essential, python3.12-dev with apt-get."),
        "{said:?}"
    );
    assert!(
        said.iter().any(|s| s == "  apt: install"),
        "what apt-get printed is passed on: {said:?}"
    );
    // order: the first look, the install, the second look, then the test
    let first = index(&events, |e| matches!(e, Event::Prerequisites { .. }));
    let installing = index(
        &events,
        |e| matches!(e, Event::Say { text } if text.starts_with("Installing ")),
    );
    let second = events
        .iter()
        .enumerate()
        .filter(|(_, e)| matches!(e, Event::Prerequisites { .. }))
        .nth(1)
        .unwrap()
        .0;
    let test = index(
        &events,
        |e| matches!(e, Event::SelfTest { state, .. } if state == "passed"),
    );
    assert!(
        first < installing && installing < second && second < test,
        "{events:#?}"
    );
}

/// Y88: a package that can be installed does not make the run install one that cannot: the driver of an old
/// series stays missing, is told after the install, and stops the run.
#[test]
fn p3_y88_what_no_package_brings_is_told_after_the_install_and_stops_the_run() {
    let c = Computer::new("y88", 0);
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer, "{message}");
    assert_eq!(
        c.apt_log().len(),
        2,
        "the packages were installed all the same"
    );
    assert!(
        message.contains("this computer lacks something the configuration needs"),
        "{message}"
    );
    assert!(
        message.contains("the driver is 535.54.03, older than 580"),
        "{message}"
    );
    assert!(
        !message.contains("a C compiler"),
        "what was put right is not told again: {message}"
    );
    assert!(
        message.contains("the packages were installed and what is listed is still missing"),
        "{message}"
    );
    assert!(
        c.python_log().is_empty(),
        "no test is run on a computer that failed the look: {events:#?}"
    );
}

/// Y89: nothing is installed that cannot or must not be: not by someone who is not the administrator and has no
/// `sudo` that needs no password, not offline, not by `check`, and not when no package of the document is for
/// this computer's package manager; each says why, with the command.
#[test]
fn p3_y89_nothing_is_installed_that_cannot_or_must_not_be_and_each_says_why() {
    // not the administrator, and no sudo at all
    let c = Computer::new("y89a", 1000);
    c.new_driver();
    let (outcome, _) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer);
    assert!(
        message.contains("is not the administrator and sudo asks for a password"),
        "{message}"
    );
    assert!(
        message.contains(
            "sudo apt-get update && sudo apt-get install -y build-essential python3.12-dev"
        ),
        "{message}"
    );
    assert!(c.apt_log().is_empty());

    // sudo that asks for a password
    write_exe(&c.bin.join("sudo"), "exit 1");
    let (outcome, _) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    assert!(failure(&outcome).1.contains("sudo asks for a password"));
    assert!(c.apt_log().is_empty());

    // sudo that does not: the packages are installed through it
    write_exe(&c.bin.join("sudo"), "shift\nexec \"$@\"");
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(
        c.apt_log(),
        [
            "update",
            "install -y -o DPkg::Lock::Timeout=120 build-essential python3.12-dev"
        ]
    );
    assert!(says(&events)
        .iter()
        .any(|s| s == "  running: sudo apt-get install -y build-essential python3.12-dev"));

    // offline: a package manager fetches
    let c = Computer::new("y89b", 0);
    c.new_driver();
    let (outcome, _) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        offline: true,
        ..c.options()
    });
    assert!(failure(&outcome)
        .1
        .contains("--offline says nothing is fetched"));
    assert!(c.apt_log().is_empty());

    // `check` changes nothing, whatever the person said
    let (outcome, _) = c.up(Options {
        dry_run: true,
        install_system_packages: true,
        ..c.options()
    });
    assert!(failure(&outcome).1.contains("`check` changes nothing"));
    assert!(c.apt_log().is_empty());

    // a computer whose package manager the document names no package for at all
    let c = Computer::new("y89c", 0);
    c.new_driver();
    std::fs::remove_file(c.bin.join("apt-get")).unwrap();
    write_exe(&c.bin.join("apk"), "exit 0");
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    let (_, message) = failure(&outcome);
    assert!(
        message.contains("no package of the document brings what is missing on this computer"),
        "{message}"
    );
    assert!(prerequisites(&events)[0].iter().all(|i| i.fix.is_none()));

    // a manager the document has a package for the compiler of and none for the headers: the compiler's is
    // installed, and what no package brings is still missing and told
    let c = Computer::new("y89d", 0);
    c.new_driver();
    std::fs::remove_file(c.bin.join("apt-get")).unwrap();
    write_exe(
        &c.bin.join("pacman"),
        &format!(
            "echo \"$*\" >> '{}'\nprintf '#!/bin/sh\\nexit 0\\n' > '{}'\nchmod +x '{}'",
            c.apt_log.display(),
            c.bin.join("gcc").display(),
            c.bin.join("gcc").display()
        ),
    );
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    let (_, message) = failure(&outcome);
    assert_eq!(c.apt_log(), ["-S --needed --noconfirm gcc"]);
    assert!(
        message.contains("the packages were installed and what is listed is still missing"),
        "{message}"
    );
    assert!(message.contains("the headers of the Python"), "{message}");
    assert!(
        !message.contains("a C compiler"),
        "what was put right is not told again: {message}"
    );
    assert!(
        !message.contains("bring what packages can"),
        "no package brings what is left: {message}"
    );
    let looks = prerequisites(&events);
    assert_eq!(
        state_of(&looks[0], "c_compiler").fix.as_deref(),
        Some("pacman -S --needed --noconfirm gcc"),
        "the command is on what a package brings: the compiler"
    );
    assert_eq!(state_of(&looks[0], "python_headers").fix, None);
    assert_eq!(state_of(&looks[1], "c_compiler").state, "met");
}

/// Y90: a package that fails to install is a failure of the run with what the package manager said last, and the
/// command to run by hand.
#[test]
fn p3_y90_a_package_that_fails_to_install_says_what_the_package_manager_said() {
    let c = Computer::new("y90", 0);
    c.new_driver();
    write_exe(
        &c.bin.join("apt-get"),
        "echo 'E: Unable to locate package python3.12-dev' >&2\nexit 100",
    );
    let (outcome, _) = c.up(Options {
        prepare_only: true,
        install_system_packages: true,
        ..c.options()
    });
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer);
    assert!(
        message.contains(
            "installing the packages failed: `apt-get install -y build-essential python3.12-dev` ended with"
        ),
        "{message}"
    );
    assert!(
        message.contains("Unable to locate package python3.12-dev"),
        "{message}"
    );
    assert!(
        message.contains("apt-get update && apt-get install -y build-essential python3.12-dev"),
        "{message}"
    );
}

/// Y91: the test of the installed environment is run after the install and before the engine is started, by the
/// Python beside the launcher, in the environment the engine is started with (the home below its data folder, the
/// Python variables of the person put away), from a folder of its own that is gone after; it is said when it
/// passes.
#[test]
fn p3_y91_the_installed_environment_is_tried_in_the_environment_the_engine_runs_in() {
    let c = Computer::new("y91", 1000);
    c.new_driver();
    c.has_compiler();
    c.has_headers();
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let log = c.python_log();
    assert_eq!(log.len(), 1, "run once: {log:?}");
    let home = c.data.join("comfyui/home");
    assert!(log[0].starts_with("ran "), "{log:?}");
    assert!(
        log[0].contains("triton_kernel.py"),
        "the application's own script: {log:?}"
    );
    assert!(
        log[0].contains(&format!("HOME={}", home.display())),
        "{log:?}"
    );
    assert!(log[0].contains("PYTHONNOUSERSITE=1"), "{log:?}");
    assert!(
        log[0].contains(&format!(
            "TMPDIR={}",
            c.data.join("comfyui/cache/tmp").display()
        )),
        "{log:?}"
    );
    // the script was put in a folder made for it, below the engine's temporary folder, and it is gone
    let left: Vec<_> = std::fs::read_dir(c.data.join("comfyui/cache/tmp"))
        .map(|d| d.flatten().map(|e| e.file_name()).collect())
        .unwrap_or_default();
    assert!(left.is_empty(), "nothing is left behind: {left:?}");
    // said as it happens: running, then passed, with what it printed
    let ends: Vec<(String, Vec<String>)> = events
        .iter()
        .filter_map(|e| match e {
            Event::SelfTest { state, output, .. } => Some((state.clone(), output.clone())),
            _ => None,
        })
        .collect();
    assert_eq!(ends.len(), 2, "{events:#?}");
    assert_eq!(ends[0].0, "running");
    assert_eq!(ends[1].0, "passed");
    assert_eq!(ends[1].1, ["ok: a kernel was built and ran"]);
    assert!(says(&events)
        .iter()
        .any(|s| s.starts_with("Testing the environment: a Triton kernel that builds")));
}

/// Y92: a test that fails stops the run with how it ended, the last lines it wrote (the compiler's, in the case
/// that made this), what the file says it is for and what the document says to do; the engine is not started,
/// and `check` and a run that was told not to look neither run it.
#[test]
fn p3_y92_a_test_that_fails_stops_the_run_before_the_engine_with_what_it_wrote() {
    let c = Computer::new("y92", 1000);
    c.new_driver();
    c.has_compiler();
    c.has_headers();
    c.selftest_ends(
        1,
        "/tmp/x.c:1:10: fatal error: Python.h: No such file or directory\nTriton could not build or run a kernel: Command returned non-zero exit status 1.",
    );
    // `up` stops at the test, and there is no engine to start (the launcher does nothing): a failure and not a hang
    let (outcome, events) = c.up(c.options());
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer, "{message}");
    assert!(
        message.contains("the environment of ComfyUI failed a test, \"a Triton kernel that builds"),
        "{message}"
    );
    assert!(
        message.contains("exit status: 1") || message.contains("exit code: 1"),
        "{message}"
    );
    assert!(
        message.contains("  | /tmp/x.c:1:10: fatal error: Python.h: No such file or directory"),
        "{message}"
    );
    assert!(
        message.contains("Proves what the two above are for."),
        "{message}"
    );
    assert!(
        message.contains("If the compiler or a header file is what is missing"),
        "{message}"
    );
    assert!(message.contains("Nothing was started."), "{message}");
    assert!(
        events
            .iter()
            .any(|e| matches!(e, Event::SelfTest { state, output, .. }
            if state == "failed" && output.iter().any(|l| l.contains("Python.h")))),
        "{events:#?}"
    );
    assert!(
        !events
            .iter()
            .any(|e| matches!(e, Event::Ready { .. } | Event::State { .. })),
        "no engine was started"
    );

    // `check` lists the test and runs none
    let before = c.python_log().len();
    let (outcome, events) = c.up(Options {
        dry_run: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Planned, "{events:#?}");
    assert_eq!(c.python_log().len(), before);
    assert_eq!(
        state_of(&prerequisites(&events)[0], "triton").state,
        "later"
    );

    // told not to look: neither the look nor the test, and a sentence says so
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        skip_prerequisites: true,
        ..c.options()
    });
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(c.python_log().len(), before, "the test was not run");
    assert!(prerequisites(&events).is_empty());
    assert!(
        says(&events).iter().any(|s| s
            .starts_with("Not looking at what this computer has to have (")
            && s.ends_with("as you said.")),
        "{:?}",
        says(&events)
    );
}

/// Y93: the Python that the headers are looked for in is the one the engine's environment is made with: the one
/// beside an installed launcher, and for a launcher that has none the check cannot be made and says so, without
/// asking another Python and without stopping the run.
#[test]
fn p3_y93_the_headers_are_looked_for_in_the_pythons_of_the_environment_or_not_at_all() {
    let c = Computer::new("y93", 1000);
    c.new_driver();
    c.has_compiler();
    std::fs::remove_file(c.scratch.join("engine/venv/bin/python")).unwrap();
    let (outcome, events) = c.up(Options {
        prepare_only: true,
        ..c.options()
    });
    // the headers are unknown, the test is skipped for want of an environment, and nothing stops the run
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let items = &prerequisites(&events)[0];
    assert_eq!(state_of(items, "python_headers").state, "unchecked");
    assert!(
        state_of(items, "python_headers")
            .detail
            .contains("there is no Python environment beside"),
        "{items:?}"
    );
    assert!(says(&events)
        .iter()
        .any(|s| s.starts_with("Not running the test \"a Triton kernel")
            && s.contains("there is no Python environment beside")));
}

/// Y94: every thing the program says about the computer is an event with a tag a script can read, and the end is
/// still the last.
#[test]
fn p3_y94_what_is_said_about_the_computer_is_json_with_tags_a_script_can_read() {
    let c = Computer::new("y94", 1000);
    c.new_driver();
    c.has_compiler();
    c.has_headers();
    let (_, events) = c.up(Options {
        prepare_only: true,
        ..c.options()
    });
    let lines: Vec<serde_json::Value> = events
        .iter()
        .map(|e| serde_json::to_value(e).unwrap())
        .collect();
    let tags: Vec<&str> = lines.iter().map(|l| l["event"].as_str().unwrap()).collect();
    assert!(tags.contains(&"prerequisites"), "{tags:?}");
    assert!(tags.contains(&"self_test"), "{tags:?}");
    assert_eq!(*tags.last().unwrap(), "finished");
    let first = lines
        .iter()
        .find(|l| l["event"] == "prerequisites")
        .unwrap();
    let compiler = &first["items"][1];
    assert_eq!(compiler["id"], "c_compiler");
    assert_eq!(compiler["state"], "met");
    assert!(
        compiler.get("fix").is_none(),
        "a key with nothing to say is left out"
    );
    let test = lines
        .iter()
        .find(|l| l["event"] == "self_test" && l["state"] == "passed")
        .unwrap();
    assert_eq!(test["id"], "triton");
    assert_eq!(test["output"][0], "ok: a kernel was built and ran");
}

/// Y99: the test of the installed environment is run before the files of the models are fetched, which are tens of
/// gigabytes: a computer that fails it is told so after the install and not after the download, and nothing is
/// fetched.
#[test]
fn p3_y99_the_test_is_run_before_the_models_are_fetched_and_a_failure_fetches_nothing() {
    use crate::testkit::headless_world::World;
    let world = World::new("y99");
    std::fs::write(
        world.config.join("engines/comfyui.toml"),
        format!(
            "schema = 1\nkind = \"engine\"\nid = \"comfyui\"\n\n[[models.lookup]]\nid = \"hub\"\nsite = \"{}\"\n",
            world.files.base()
        ),
    )
    .unwrap();
    let engine = world.scratch.join("engine");
    let launcher = engine.join("comfyui");
    write_exe(&launcher, "exit 0");
    let include = world.scratch.join("include");
    let log = world.scratch.join("python.log");
    python_beside(&launcher, &include, &log, &world.scratch);
    world.files.add(
        "/acme/Big/resolve/main/big.safetensors",
        b"the weights".to_vec(),
    );
    let file = world.scratch.join("big.toml");
    std::fs::write(
        &file,
        "schema = 1\nkind = \"profile\"\nid = \"big\"\nname = \"Big\"\nengine = \"comfyui\"\n\n[[model]]\nfile = \"big.safetensors\"\nfrom = \"huggingface:acme/Big\"\nsize = 11\n\n[[needs]]\nsystem = \"triton\"\n",
    )
    .unwrap();
    let options = || Options {
        prepare_only: true,
        accept_review: true,
        gpus: Some(vec![GpuVendor::Nvidia]),
        sets: vec![format!("comfyui.bin_override={}", launcher.display())],
        machine: Some(Machine {
            path: Vec::new(),
            root: world.scratch.join("root"),
            vars: Default::default(),
            uid: 1000,
        }),
        ..Options::default()
    };
    let models = world
        .data
        .join("comfyui/models/diffusion_models/big.safetensors");

    // it fails: the weights are not fetched, and not asked for
    std::fs::write(world.scratch.join("selftest.code"), "1").unwrap();
    std::fs::write(world.scratch.join("selftest.says"), "no compiler\n").unwrap();
    let (outcome, events) = world.up(&file, options());
    let (kind, message) = failure(&outcome);
    assert_eq!(*kind, FailureKind::Computer, "{events:#?}");
    assert!(message.contains("no compiler"), "{message}");
    assert_eq!(world.files.requests(), 0, "nothing was fetched");
    assert!(!models.exists());
    assert!(
        !events.iter().any(|e| matches!(e, Event::Step { .. })),
        "no step was begun"
    );

    // it passes: the test comes before the step that fetches
    std::fs::remove_file(world.scratch.join("selftest.code")).unwrap();
    std::fs::remove_file(world.scratch.join("selftest.says")).unwrap();
    let (outcome, events) = world.up(&file, options());
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    assert_eq!(std::fs::read(&models).unwrap(), b"the weights");
    let test = index(
        &events,
        |e| matches!(e, Event::SelfTest { state, .. } if state == "passed"),
    );
    let fetch = index(&events, |e| matches!(e, Event::Step { .. }));
    assert!(test < fetch, "{events:#?}");
}

/// Y99b: when what is missing is only what no package brings (a driver), the sentence does not offer to install
/// anything, for `check` as for a run, and still says how to leave the look out.
#[test]
fn p3_y99b_what_no_package_brings_is_not_offered_an_install() {
    let c = Computer::new("y99b", 0);
    c.has_compiler();
    c.has_headers();
    for (options, mentions_check) in [
        (
            Options {
                dry_run: true,
                ..c.options()
            },
            true,
        ),
        (
            Options {
                prepare_only: true,
                ..c.options()
            },
            false,
        ),
    ] {
        let (outcome, _) = c.up(options);
        let (kind, message) = failure(&outcome);
        assert_eq!(*kind, FailureKind::Computer, "{message}");
        assert!(
            message.contains("the driver is 535.54.03, older than 580"),
            "{message}"
        );
        assert!(
            !message.contains("--install-system-packages"),
            "nothing to install: {message}"
        );
        assert!(!message.contains("bring what packages can"), "{message}");
        assert!(
            message.contains("--skip-prerequisites leaves this look out."),
            "{message}"
        );
        assert_eq!(
            message.contains("`check` changes nothing"),
            mentions_check,
            "{message}"
        );
    }
    assert!(c.apt_log().is_empty());
}

/// Y99d: through the whole run, an environment that is made is not asked whether its Python could make one: the
/// Python beside the launcher lacks `ensurepip` (as a venv made with `--without-pip` does) and the look says the
/// environment is made, without asking.
#[test]
fn p3_y99d_an_environment_that_is_made_is_not_asked_for_the_module_that_made_it() {
    let c = Computer::new("y99d", 1000);
    let asked = c.scratch.join("ensurepip-asked");
    // the Python beside the launcher: facts when asked for them, and a Python without the module when asked for it
    write_exe(
        &c.scratch.join("engine/venv/bin/python"),
        &format!(
            "case \"$3\" in\n*sysconfig*) printf '3 12\\n{include}\\n/usr\\n'; exit 0;;\n*ensurepip*) echo asked >> '{asked}'; echo \"ModuleNotFoundError: No module named 'ensurepip'\" >&2; exit 1;;\nesac\nexit 0",
            include = c.include.display(),
            asked = asked.display()
        ),
    );
    let path = c.scratch.join("venv-only.toml");
    std::fs::write(
        &path,
        "schema = 1\nkind = \"profile\"\nid = \"venv-only\"\nname = \"V\"\nengine = \"comfyui\"\n\n[[needs]]\nsystem = \"python_venv\"\n",
    )
    .unwrap();
    let mut events = Vec::new();
    let request = Request {
        dirs: ConfigDirs {
            user: Some(c.config.clone()),
            data: Some(c.data.clone()),
            ..ConfigDirs::default()
        },
        target: path.display().to_string(),
        options: Options {
            prepare_only: true,
            ..c.options()
        },
    };
    let outcome = provision::up(&request, &mut |e| events.push(e), &AtomicBool::new(false));
    assert_eq!(outcome, Outcome::Prepared, "{events:#?}");
    let item = &prerequisites(&events)[0][0];
    assert_eq!(item.id, "python_venv");
    assert_eq!(item.state, "met");
    assert_eq!(item.detail, "the environment is made");
    assert!(
        !asked.exists(),
        "the Python of an environment that is made was not asked"
    );
}

/// Y89b: a stop before the install is a stop (code 130), and not a failure of the packages: no command is run.
#[test]
fn p3_y89b_a_stop_before_the_install_is_an_interruption_and_installs_nothing() {
    let c = Computer::new("y89bb", 0);
    c.new_driver();
    let request = Request {
        dirs: ConfigDirs {
            user: Some(c.config.clone()),
            data: Some(c.data.clone()),
            ..ConfigDirs::default()
        },
        target: c.configuration(NEEDS).display().to_string(),
        options: Options {
            prepare_only: true,
            install_system_packages: true,
            ..c.options()
        },
    };
    let mut events = Vec::new();
    let outcome = provision::up(&request, &mut |e| events.push(e), &AtomicBool::new(true));
    assert_eq!(outcome, Outcome::Interrupted, "{events:#?}");
    assert_eq!(outcome.exit_code(), 130);
    assert!(c.apt_log().is_empty(), "no command was run");
}
