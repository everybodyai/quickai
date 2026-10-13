//! Group F of the test plan (work package 4.11): ComfyUI as an engine, from the installer to the
//! session. The document and the plan are checked in `quickai-core/tests/p3_comfy.rs`; what is here
//! needs processes and servers, and a Python that is a shell script, so that a person's Python and
//! the PyTorch it would fetch are not needed to know that the install does what it says:
//!
//! * the fake Python answers the question of its version, makes a "virtual environment" that is a
//!   copy of itself, and writes down what pip is asked (and what it is run with);
//! * the archives of the source and of a node pack are real tarballs, served by a real local server;
//! * the session starts a script that records its arguments, and an HTTP fake answers for ComfyUI.

use std::path::{Path, PathBuf};
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use quickai_core::doc::engine::Verify;

use crate::jobs::JobEvent;
use crate::prims::install::{
    find_python, install_blocking, is_unfinished, list_installed, list_unfinished, InstallKind,
    InstallRequest, InstallSpec, NodePlan, PipPlan, PythonApp, Runs, Smoke, Source, Where,
    CANCELLED,
};
use crate::testkit::archives::{sha256_hex, tar_gz, Entry};
use crate::testkit::files::FileServer;
use crate::testkit::procs::{gone_within, write_script};
use crate::testkit::{scratch, wait_until};

// ---- the fake Python ------------------------------------------------------------------------------------

/// A Python that is a shell script. `mode` is how its pip behaves: `ok`, `fail` (a pip-like error),
/// `hang` (it starts a child that sleeps and waits for it), `smoke_fail` (the program it runs ends with 3)
/// or `smoke_hang` (the program it runs starts a child that sleeps and waits for it); a file beside the log (`World::mode`) changes it between two installs. Everything it
/// is asked is written to `log`; a venv it makes is a copy of it.
#[cfg(unix)]
pub(super) fn fake_python(dir: &Path, version: &str, mode: &str) -> (PathBuf, PathBuf) {
    let log = dir.join("python.log");
    let body = format!(
        r#"LOG='{log}'
MODE='{mode}'
[ -f "$LOG.mode" ] && MODE=$(cat "$LOG.mode")
case "$1" in
  -c)
    case "$2" in
      # which function a console script of the wheel calls: what the package's records say
      *entry_points*) echo "entry $3" >> "$LOG"; echo "open_webui:app"; exit 0;;
      # the application of a wheel, run by a launcher that was told what to call
      *open_webui*)
        shift 2
        echo "run open-webui $*" >> "$LOG"
        if [ "$MODE" = smoke_fail ]; then echo 'ImportError: no module' >&2; exit 3; fi
        exit 0;;
      *) echo "{version}"; exit 0;;
    esac;;
  -m)
    case "$2" in
      venv)
        mkdir -p "$3/bin" || exit 1
        cp "$0" "$3/bin/python" && chmod +x "$3/bin/python" || exit 1
        echo "home = fake" > "$3/pyvenv.cfg"
        echo "venv $3" >> "$LOG"
        exit 0;;
      pip)
        shift 2
        echo "pip TMPDIR=$TMPDIR $*" >> "$LOG"
        echo "$0" >> "$LOG.who"
        echo "Collecting torch"
        case "$MODE" in
          fail)
            echo "ERROR: No matching distribution found for torch" >&2
            exit 1;;
          hang)
            sleep 60 &
            echo $! > "$LOG.sleeper"
            wait;;
          *)
            echo "  Downloading https://example.invalid/torch-9.9.9-cp3-none-any.whl (6224.9 MB)"
            echo "Installing collected packages: torch"
            echo "Successfully installed torch-9.9.9"
            exit 0;;
        esac;;
    esac;;
  *)
    echo "run $*" >> "$LOG"
    if [ "$MODE" = smoke_fail ]; then echo "ImportError: no torch" >&2; exit 3; fi
    if [ "$MODE" = smoke_hang ]; then
      sleep 30 &
      echo $! > "$LOG.sleeper"
      wait
    fi
    exit 0;;
esac"#,
        log = log.display(),
    );
    (write_script(dir, "python3.98", &body), log)
}

pub(super) fn lines(log: &Path) -> Vec<String> {
    std::fs::read_to_string(log)
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// What is directly under `root`, sorted: what an install leaves behind shows here.
pub(super) fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

// ---- what is installed from ---------------------------------------------------------------------------------

fn source_tar() -> Vec<u8> {
    tar_gz(&[
        Entry::file("Comfy-9.9.9/main.py", b"print('comfy')\n"),
        Entry::file("Comfy-9.9.9/requirements.txt", b"torch\n"),
        Entry::file("Comfy-9.9.9/custom_nodes/example.py", b"\n"),
    ])
}

fn node_tar() -> Vec<u8> {
    tar_gz(&[
        Entry::file("Node-abc123/__init__.py", b"NODE_CLASS_MAPPINGS = {}\n"),
        Entry::file("Node-abc123/requirements.txt", b"gguf\n"),
    ])
}

struct World {
    server: FileServer,
    dir: PathBuf,
    python: PathBuf,
    log: PathBuf,
    source: Vec<u8>,
    node: Vec<u8>,
}

impl World {
    fn new(name: &str, mode: &str) -> World {
        let dir = scratch(name).into_path();
        let (python, log) = fake_python(&dir, "3 98", mode);
        let server = FileServer::start();
        let (source, node) = (source_tar(), node_tar());
        server.add("/v9.tar.gz", source.clone());
        server.add("/gguf.tar.gz", node.clone());
        World {
            server,
            dir,
            python,
            log,
            source,
            node,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("builds")
    }

    /// How the Python behaves from now on (also the one a venv made earlier is a copy of).
    fn mode(&self, mode: &str) {
        std::fs::write(format!("{}.mode", self.log.display()), mode).unwrap();
    }

    fn spec(&self) -> InstallSpec {
        let pip = |id: &str,
                   packages: &[&str],
                   requirements: Option<&str>,
                   index: Option<&str>,
                   weight| PipPlan {
            id: id.into(),
            label: format!("the {id} step"),
            packages: packages.iter().map(|p| p.to_string()).collect(),
            requirements: requirements.map(str::to_string),
            wheel: false,
            index: index.map(str::to_string),
            weight,
        };
        InstallSpec {
            root: self.root(),
            binary: "comfyui".into(),
            search_depth: 0,
            chmod: true,
            fallback_label: "comfyui-build".into(),
            verify: Verify::Digest,
            smoke: Some(Smoke {
                args: vec!["--quick-test-for-ci".into(), "--cpu".into()],
                timeout: Duration::from_secs(20),
            }),
            kind: InstallKind::PythonApp(Box::new(PythonApp {
                runs: Runs::Script("main.py".into()),
                versions: vec!["3.98".into()],
                python: Some(self.python.clone()),
                pip: vec![
                    pip(
                        "torch",
                        &["torch", "torchvision"],
                        None,
                        Some("https://example.invalid/whl/rocm"),
                        10,
                    ),
                    pip("requirements", &[], Some("requirements.txt"), None, 4),
                ],
                nodes: vec![NodePlan {
                    id: "gguf".into(),
                    label: "ComfyUI-GGUF".into(),
                    url: self.server.url("/gguf.tar.gz"),
                    sha256: format!("sha256:{}", sha256_hex(&self.node)),
                    into: "custom_nodes/gguf".into(),
                    requirements: Some("requirements.txt".into()),
                }],
                space: None,
            })),
            ..Default::default()
        }
    }

    fn request(&self, pin: Option<String>) -> InstallRequest {
        InstallRequest {
            label: "v9".into(),
            base: Source {
                url: self.server.url("/v9.tar.gz"),
                host_digest: None,
                pin,
                size: None,
            },
            overlays: Vec::new(),
        }
    }

    fn pinned(&self) -> InstallRequest {
        self.request(Some(format!("sha256:{}", sha256_hex(&self.source))))
    }
}

impl Drop for World {
    fn drop(&mut self) {
        // what a test left running (a hung pip's sleeper) goes before its directory does
        if let Ok(text) = std::fs::read_to_string(format!("{}.sleeper", self.log.display())) {
            if let Ok(pid) = text.trim().parse() {
                crate::testkit::procs::kill_quietly(pid);
            }
        }
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

/// What an install did.
pub(super) struct Ran {
    pub outcome: Result<PathBuf, String>,
    pub said: Vec<String>,
    pub bar: Vec<f32>,
    /// The lines under the bar, in order.
    pub details: Vec<String>,
}

pub(super) fn run(spec: &InstallSpec, request: &InstallRequest) -> Ran {
    let events = Mutex::new((Vec::new(), Vec::new(), Vec::new()));
    let outcome = install_blocking(spec, request, &|event| {
        let mut events = events.lock().unwrap();
        match event {
            JobEvent::Status(line) => events.0.push(line),
            JobEvent::Fraction(value) => events.1.push(value),
            JobEvent::Detail(text) => events.2.push(text),
            _ => {}
        }
    });
    let (said, bar, details) = events.into_inner().unwrap();
    Ran {
        outcome,
        said,
        bar,
        details,
    }
}

pub(super) fn said(ran: &Ran, word: &str) -> bool {
    ran.said.iter().any(|line| line.contains(word))
}

// ---- finding a Python ----------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_f20_a_python_is_found_by_its_version_in_the_places_pythons_are_kept() {
    let dir = scratch("f20").into_path();
    let (a, b, c, home) = (
        dir.join("path-a"),
        dir.join("path-b"),
        dir.join("path-c"),
        dir.join("home"),
    );
    for d in [&a, &b, &c] {
        std::fs::create_dir_all(d).unwrap();
    }
    // 3.98 on the PATH answers that it is 3.97; 3.97 is a broken program; a third answers right
    write_script(&a, "python3.98", "echo '3 97'");
    write_script(&b, "python3.97", "exit 1");
    write_script(&c, "python3.97", "echo '3 97'");
    let at = |home: Option<&Path>| Where {
        path_var: Some(std::env::join_paths([&a, &b, &c]).unwrap()),
        home: home.map(Path::to_path_buf),
    };
    let versions = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();

    // the first of the versions that really is that version, whatever else the PATH holds
    let found = find_python(&versions(&["3.98", "3.97"]), None, &at(None)).unwrap();
    assert_eq!(found.version, (3, 97));
    assert_eq!(found.program, c.join("python3.97"));
    assert!(
        found.describe().starts_with("Python 3.97 ("),
        "{}",
        found.describe()
    );

    // pyenv keeps its Pythons by version, and the newest patch release is the first tried
    let pyenv = home.join(".pyenv/versions");
    for patch in ["3.96.2", "3.96.10", "3.95.1"] {
        std::fs::create_dir_all(pyenv.join(patch).join("bin")).unwrap();
        write_script(
            &pyenv.join(patch).join("bin"),
            "python",
            &format!("echo '3 {}'", &patch[2..4]),
        );
    }
    let found = find_python(&versions(&["3.96"]), None, &at(Some(&home))).unwrap();
    assert_eq!(found.version, (3, 96));
    assert!(
        found.program.to_string_lossy().contains("3.96.10"),
        "{}",
        found.program.display()
    );

    // one the person names is used, if it is not older than the document's oldest
    let named = find_python(&versions(&["3.98"]), Some(&c.join("python3.97")), &at(None));
    assert!(named
        .unwrap_err()
        .contains("is 3.97, and this needs 3.98 or newer"));
    let named = find_python(
        &versions(&["3.96", "3.97"]),
        Some(&c.join("python3.97")),
        &at(None),
    )
    .unwrap();
    assert_eq!(named.version, (3, 97));
    let broken =
        find_python(&versions(&["3.97"]), Some(&b.join("python3.97")), &at(None)).unwrap_err();
    assert!(
        broken.contains("the Python you chose") && broken.contains("ended with"),
        "{broken}"
    );

    // none: the sentence says which versions and what to do about it
    let none = find_python(&versions(&["3.94", "3.93"]), None, &at(Some(&home))).unwrap_err();
    assert!(
        none.contains("no Python 3.94 or 3.93 was found")
            && none.contains("python3.12-venv")
            && none.contains("Python box of Settings"),
        "{none}"
    );
    let _ = std::fs::remove_dir_all(dir);
}

// ---- the install --------------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_f21_a_python_app_is_installed_from_its_source_with_an_environment_and_a_launcher() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("f21", "ok");
    let spec = world.spec();
    let ran = run(&spec, &world.pinned());
    let launcher = ran
        .outcome
        .clone()
        .unwrap_or_else(|e| panic!("{e}\n{:#?}", ran.said));
    let root = world.root();

    // the build is whole, and nothing of the install is left beside it
    assert_eq!(launcher, root.join("v9").join("comfyui"));
    assert_eq!(entries(&root), ["v9"]);
    let build = root.join("v9");
    assert_eq!(
        entries(&build),
        ["app", "comfyui", "venv"],
        "the temporary files went"
    );
    assert!(build.join("app/main.py").is_file() && build.join("app/requirements.txt").is_file());
    assert!(
        build.join("app/custom_nodes/gguf/__init__.py").is_file(),
        "the node pack is in the source, under the folder the document names"
    );
    assert!(build.join("venv/bin/python").is_file() && build.join("venv/pyvenv.cfg").is_file());
    let mode = std::fs::metadata(&launcher).unwrap().permissions().mode();
    assert_ne!(mode & 0o111, 0, "the launcher can be run");
    let text = std::fs::read_to_string(&launcher).unwrap();
    assert!(
        text.starts_with("#!/bin/sh")
            && text.contains(r#"exec "$here/venv/bin/python" "$here/app/main.py" "$@""#),
        "{text}"
    );

    // what it asked of the Python, in order: an environment, PyTorch from its index alone and only as
    // wheels, what the source needs, what the pack needs, and the check
    let log = lines(&world.log);
    let staging = format!("{}/v9.partial", root.display());
    assert_eq!(log.len(), 5, "{log:#?}");
    assert_eq!(log[0], format!("venv {staging}/venv"));
    assert!(
        log[1].starts_with(&format!("pip TMPDIR={staging}/tmp install --no-input"))
            && log[1].contains("--only-binary=:all:")
            && log[1].contains("--index-url https://example.invalid/whl/rocm torch torchvision")
            && log[1].ends_with("--no-cache-dir"),
        "{}",
        log[1]
    );
    assert!(
        log[2].contains(&format!("-r {staging}/app/requirements.txt"))
            && !log[2].contains("--index-url"),
        "{}",
        log[2]
    );
    assert!(
        log[3].contains(&format!(
            "-r {staging}/app/custom_nodes/gguf/requirements.txt"
        )),
        "{}",
        log[3]
    );
    assert!(
        log[4].ends_with("/app/main.py --quick-test-for-ci --cpu"),
        "the check runs the launcher's program with the document's words: {}",
        log[4]
    );

    // the log follows the steps, and says what was checked
    for word in [
        "Using Python 3.98",
        "Checked v9 against its sha256",
        "Checked ComfyUI-GGUF against its sha256",
        "Making the Python environment",
        "Installing the torch step",
        "Installing the requirements step",
        "Installing what ComfyUI-GGUF needs",
        "Successfully installed torch-9.9.9",
        "Checking that it runs",
    ] {
        assert!(said(&ran, word), "`{word}` is not in {:#?}", ran.said);
    }

    // the line under the bar says which step it is on, from the first
    let titles: Vec<&str> = ran
        .details
        .iter()
        .map(String::as_str)
        .filter(|d| d.starts_with("Step "))
        .collect();
    assert_eq!(
        titles,
        [
            "Step 1 of 7 · the source of v9",
            "Step 2 of 7 · ComfyUI-GGUF",
            "Step 3 of 7 · the Python environment",
            "Step 4 of 7 · the torch step",
            "Step 5 of 7 · the requirements step",
            "Step 6 of 7 · what ComfyUI-GGUF needs",
            "Step 7 of 7 · the check that it runs (it loads every node; the first time can take a minute)",
        ]
    );

    // the bar only goes forward, and is not full until the install is whole
    assert!(ran.bar.len() > 6, "{:?}", ran.bar);
    assert!(
        ran.bar.windows(2).all(|w| w[0] <= w[1] + 1e-6),
        "it went back: {:?}",
        ran.bar
    );
    assert!(
        ran.bar.iter().all(|v| (0.0..1.0).contains(v)),
        "{:?}",
        ran.bar
    );
    assert!(*ran.bar.last().unwrap() > 0.9, "{:?}", ran.bar);

    // the launcher still runs where the build was moved to
    let status = std::process::Command::new(&launcher)
        .args(["--port", "1"])
        .status()
        .unwrap();
    assert!(status.success());
    assert!(
        lines(&world.log)
            .last()
            .unwrap()
            .ends_with(&format!("{}/v9/app/main.py --port 1", root.display())),
        "{:#?}",
        lines(&world.log)
    );

    // and the runtime manager finds it
    let builds = list_installed(&spec);
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].label, "v9");
    assert_eq!(builds[0].binary_path, launcher);

    // an install of what is there needs nothing from anyone
    let asked = world.server.requests();
    let again = run(&spec, &world.pinned());
    assert_eq!(again.outcome, Ok(launcher));
    assert_eq!(world.server.requests(), asked);
}

#[cfg(unix)]
#[test]
fn p3_f22_a_source_that_is_not_the_one_tested_is_refused_before_it_is_unpacked_and_an_unpinned_one_says_so(
) {
    let world = World::new("f22", "ok");
    // the pin is not the file's: refused, nothing unpacked, nothing asked of the Python
    let wrong = world.request(Some(format!("sha256:{}", "ab".repeat(32))));
    let ran = run(&world.spec(), &wrong);
    let error = ran.outcome.clone().unwrap_err();
    assert!(
        error.contains("does not match its checksum") && error.contains("abababababab"),
        "{error}"
    );
    assert_eq!(
        entries(&world.root()),
        Vec::<String>::new(),
        "nothing is left"
    );
    assert!(
        lines(&world.log).is_empty(),
        "no environment was made for a file that was refused"
    );

    // a release that was not the tested one has no pin: it is installed, and the log says it was not checked
    let ran = run(&world.spec(), &world.request(None));
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(said(&ran, "is not verified"), "{:#?}", ran.said);

    // a node pack is pinned always: a pack that is not its commit's is refused, with the source unpacked
    // and the environment not yet made
    let world = World::new("f22b", "ok");
    let mut spec = world.spec();
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.nodes[0].sha256 = format!("sha256:{}", "cd".repeat(32));
    let ran = run(&spec, &world.pinned());
    let error = ran.outcome.clone().unwrap_err();
    assert!(
        error.contains("ComfyUI-GGUF does not match its checksum"),
        "{error}"
    );
    assert!(entries(&world.root()).is_empty());
    assert!(lines(&world.log).is_empty());
}

#[cfg(unix)]
#[test]
fn p3_f23_a_pip_that_fails_says_what_it_said_and_keeps_the_work_to_go_on_from() {
    let world = World::new("f23", "fail");
    let spec = world.spec();
    let ran = run(&spec, &world.pinned());
    let error = ran.outcome.clone().unwrap_err();
    assert!(
        error.starts_with("pip ended with")
            && error.contains("ERROR: No matching distribution found for torch"),
        "{error}"
    );
    assert!(said(&ran, "Collecting torch"), "{:#?}", ran.said);
    assert_eq!(
        lines(&world.log)
            .iter()
            .filter(|l| l.starts_with("pip "))
            .count(),
        1,
        "the steps after the one that failed were not run"
    );

    // the source, the pack and the environment are done, and kept as an install that is not whole
    assert!(said(&ran, "The work so far is kept in"), "{:#?}", ran.said);
    assert_eq!(entries(&world.root()), ["v9.partial"]);
    assert!(is_unfinished(&world.root().join("v9.partial")));
    assert!(list_installed(&spec).is_empty(), "it is not a build");
    let stopped = list_unfinished(&world.root());
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_eq!(
        (stopped[0].label.as_str(), stopped[0].done, stopped[0].total),
        ("v9", 3, 7)
    );
    assert_eq!(stopped[0].next.as_deref(), Some("the torch step"));
    assert!(stopped[0].size_bytes > 0);
}

#[cfg(unix)]
#[test]
fn p3_f24_an_install_that_is_cancelled_stops_pip_and_what_it_started_and_leaves_nothing() {
    let world = World::new("f24", "hang");
    let spec = world.spec();
    let cancel = spec.cancel.clone();
    let request = world.pinned();
    let handle = std::thread::spawn(move || run(&spec, &request));

    // pip is running, and has started something of its own
    let sleeper = format!("{}.sleeper", world.log.display());
    wait_until(Duration::from_secs(10), "pip to start its child", || {
        std::fs::read_to_string(&sleeper).is_ok_and(|t| t.trim().parse::<u32>().is_ok())
    });
    let pid: u32 = std::fs::read_to_string(&sleeper)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    let asked = Instant::now();
    cancel.store(true, Ordering::SeqCst);
    let ran = handle.join().unwrap();

    assert_eq!(ran.outcome, Err(CANCELLED.to_string()));
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "it took {:?}",
        asked.elapsed()
    );
    assert!(
        gone_within(pid, Duration::from_secs(5)),
        "what pip started was left running"
    );
    assert_eq!(entries(&world.root()), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn p3_f24b_an_install_that_is_cancelled_while_its_check_runs_stops_the_check_at_once() {
    let world = World::new("f24b", "smoke_hang");
    let spec = world.spec();
    let cancel = spec.cancel.clone();
    let request = world.pinned();
    let handle = std::thread::spawn(move || run(&spec, &request));

    // the check is running: the launcher was run, and it waits for something that takes half a minute
    let sleeper = format!("{}.sleeper", world.log.display());
    wait_until(Duration::from_secs(10), "the check to start", || {
        std::fs::read_to_string(&sleeper).is_ok_and(|t| t.trim().parse::<u32>().is_ok())
    });
    let asked = Instant::now();
    cancel.store(true, Ordering::SeqCst);
    let ran = handle.join().unwrap();

    assert_eq!(ran.outcome, Err(CANCELLED.to_string()));
    assert!(
        asked.elapsed() < Duration::from_secs(5),
        "it took {:?}",
        asked.elapsed()
    );
    assert_eq!(entries(&world.root()), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn p3_f25_an_install_that_will_not_fit_or_has_no_python_is_refused_before_anything_is_fetched() {
    let world = World::new("f25", "ok");
    // room: the document says what it needs, and the disk has less
    let mut spec = world.spec();
    spec.free_space = |_| Some(5 * 1024 * 1024 * 1024);
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.space = Some(30 * 1024 * 1024 * 1024);
    let error = run(&spec, &world.pinned()).outcome.unwrap_err();
    assert!(
        error.contains("not enough free space")
            && error.contains("30.0 GB")
            && error.contains("5.0 GB"),
        "{error}"
    );
    assert_eq!(
        world.server.requests(),
        0,
        "nothing was asked of the network"
    );

    // a Python: none of the document's versions is here
    let mut spec = world.spec();
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.python = None;
    app.versions = vec!["3.97".into(), "3.96".into()];
    let error = run(&spec, &world.pinned()).outcome.unwrap_err();
    assert!(
        error.contains("no Python 3.97 or 3.96 was found"),
        "{error}"
    );
    assert_eq!(world.server.requests(), 0);
    assert_eq!(entries(&world.root()), Vec::<String>::new());
}

#[cfg(unix)]
#[test]
fn p3_f26_a_build_that_does_not_run_is_not_installed_and_a_stale_staging_directory_is_replaced() {
    let world = World::new("f26", "smoke_fail");
    let spec = world.spec();
    let error = run(&spec, &world.pinned()).outcome.unwrap_err();
    assert!(
        error.contains("failed its check") && error.contains("ImportError: no torch"),
        "{error}"
    );
    // it is not a build; what was done is kept, so that when the cause is out of the way only the
    // check is run again
    assert!(list_installed(&spec).is_empty());
    assert_eq!(entries(&world.root()), ["v9.partial"]);
    world.mode("ok");
    let before = lines(&world.log).len();
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    let after = lines(&world.log)[before..].to_vec();
    assert_eq!(after.len(), 1, "{after:#?}");
    assert!(after[0].starts_with("run ") && after[0].ends_with("--quick-test-for-ci --cpu"));
    assert_eq!(entries(&world.root()), ["v9"]);

    // what an install killed in the middle left, with no record of what was done, is not a build, and
    // the next install clears it
    let world = World::new("f26b", "ok");
    let stale = world.root().join("v9.partial");
    std::fs::create_dir_all(stale.join("venv")).unwrap();
    std::fs::write(stale.join("venv/junk"), b"x").unwrap();
    assert!(
        list_installed(&world.spec()).is_empty(),
        "a staging directory is not a build"
    );
    assert!(
        list_unfinished(&world.root()).is_empty(),
        "and with no record it cannot be gone on with"
    );
    let ran = run(&world.spec(), &world.pinned());
    assert!(ran.outcome.is_ok(), "{:?}", ran.outcome);
    assert_eq!(entries(&world.root()), ["v9"]);
    assert!(!world.root().join("v9/venv/junk").exists());
}

#[cfg(unix)]
#[test]
fn p3_f27_an_archive_that_is_not_the_source_of_one_folder_is_refused() {
    let world = World::new("f27", "ok");
    // two folders at the top: a source archive has one
    world.server.add(
        "/two.tar.gz",
        tar_gz(&[Entry::file("a/x", b"1"), Entry::file("b/y", b"2")]),
    );
    let mut request = world.request(None);
    request.base.url = world.server.url("/two.tar.gz");
    let error = run(&world.spec(), &request).outcome.unwrap_err();
    assert!(
        error.contains("does not have the shape of a source archive"),
        "{error}"
    );
    assert_eq!(entries(&world.root()), Vec::<String>::new());
    // and one that is not an archive this installer reads is refused before any request
    let mut request = world.request(None);
    request.base.url = world.server.url("/v9.7z");
    let error = run(&world.spec(), &request).outcome.unwrap_err();
    assert!(error.contains("don't know how to extract"), "{error}");
}

// ---- an install that stops ------------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_f70_an_install_that_stopped_goes_on_from_the_steps_it_had_done() {
    let world = World::new("f70", "fail");
    let spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    let asked = world.server.requests();

    // the cause is out of the way: the next install does the steps after the one that failed, and nothing before
    world.mode("ok");
    let before = lines(&world.log).len();
    let ran = run(&spec, &world.pinned());
    let launcher = ran
        .outcome
        .clone()
        .unwrap_or_else(|e| panic!("{e}\n{:#?}", ran.said));
    assert_eq!(launcher, world.root().join("v9").join("comfyui"));
    assert_eq!(world.server.requests(), asked, "nothing was fetched again");
    let after = lines(&world.log)[before..].to_vec();
    assert!(
        !after.iter().any(|l| l.starts_with("venv ")),
        "the environment was not made again: {after:#?}"
    );
    assert_eq!(
        after.iter().filter(|l| l.starts_with("pip ")).count(),
        3,
        "PyTorch, the requirements and what the pack needs: {after:#?}"
    );
    assert!(after.last().unwrap().ends_with("--quick-test-for-ci --cpu"));
    for word in [
        "Going on with the install that stopped (3 of 7 steps are done)",
        "Already done by the install that stopped: the source of v9",
        "Already done by the install that stopped: ComfyUI-GGUF",
        "Already done by the install that stopped: the Python environment",
    ] {
        assert!(said(&ran, word), "`{word}` is not in {:#?}", ran.said);
    }
    assert!(
        !said(&ran, "Downloading v9")
            && !said(&ran, "Downloading ComfyUI-GGUF")
            && !said(&ran, "Making the Python environment"),
        "{:#?}",
        ran.said
    );

    // the bar counts the steps that were done, and only goes forward
    assert!(
        ran.bar.windows(2).all(|w| w[0] <= w[1] + 1e-6),
        "{:?}",
        ran.bar
    );
    assert!(
        ran.bar[1] > 0.1,
        "the first step was counted: {:?}",
        ran.bar
    );

    // whole at last: the record of the install is gone with everything else that was not the build
    assert_eq!(entries(&world.root()), ["v9"]);
    assert_eq!(
        entries(&world.root().join("v9")),
        ["app", "comfyui", "venv"]
    );
    assert!(list_unfinished(&world.root()).is_empty());
    assert_eq!(list_installed(&spec).len(), 1);
}

#[cfg(unix)]
#[test]
fn p3_f71_an_install_that_stopped_for_another_plan_is_started_again() {
    let world = World::new("f71", "fail");
    assert!(run(&world.spec(), &world.pinned()).outcome.is_err());
    world.mode("ok");

    // the packages are not the ones the environment on disk was made for (an update of the application,
    // another Python): it is not the environment this install would make
    let mut spec = world.spec();
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.pip[0].packages = vec!["torch".into()];
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(said(&ran, "made for other things"), "{:#?}", ran.said);
    assert!(
        said(&ran, "Making the Python environment"),
        "{:#?}",
        ran.said
    );
    assert_eq!(
        lines(&world.log)
            .iter()
            .filter(|l| l.starts_with("venv "))
            .count(),
        2,
        "the environment was made twice"
    );
    assert_eq!(entries(&world.root()), ["v9"]);
}

#[cfg(unix)]
#[test]
fn p3_f72_an_install_that_is_going_on_is_not_cleared_or_gone_on_with_by_another_window() {
    let world = World::new("f72", "hang");
    let spec = world.spec();
    let cancel = spec.cancel.clone();
    let request = world.pinned();
    let first = std::thread::spawn(move || run(&spec, &request));
    let sleeper = format!("{}.sleeper", world.log.display());
    wait_until(Duration::from_secs(10), "pip to start its child", || {
        std::fs::read_to_string(&sleeper).is_ok_and(|t| t.trim().parse::<u32>().is_ok())
    });

    // another window asks for the same build: it is told, and what the first has done is where it was
    let second = run(&world.spec(), &world.pinned());
    let error = second.outcome.unwrap_err();
    assert!(
        error.contains("is being installed by another window of QuickAI"),
        "{error}"
    );
    assert!(world.root().join("v9.partial/venv/bin/python").is_file());
    assert!(
        list_unfinished(&world.root()).is_empty(),
        "it is going on: it is not one that stopped"
    );

    cancel.store(true, Ordering::SeqCst);
    let ran = first.join().unwrap();
    assert_eq!(ran.outcome, Err(CANCELLED.to_string()));
    assert_eq!(
        entries(&world.root()),
        Vec::<String>::new(),
        "and the person's cancel throws the work away, as it says"
    );
}

#[cfg(unix)]
#[test]
fn p3_f73_an_install_renamed_by_hand_is_not_a_build_and_is_gone_on_with() {
    let world = World::new("f73", "fail");
    let spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    let root = world.root();

    // what a person does who sees an environment that looks whole: it is not a build, whatever its name
    std::fs::rename(root.join("v9.partial"), root.join("v9")).unwrap();
    assert!(list_installed(&spec).is_empty());
    let stopped = list_unfinished(&root);
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    assert_eq!(stopped[0].dir, root.join("v9"));

    // and the install of that build goes on from it, whatever it was called
    world.mode("ok");
    let before = lines(&world.log).len();
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(said(&ran, "Going on with the install that stopped"));
    assert!(!lines(&world.log)[before..]
        .iter()
        .any(|l| l.starts_with("venv ")));
    assert_eq!(entries(&root), ["v9"]);
    assert!(!is_unfinished(&root.join("v9")));
    assert_eq!(list_installed(&spec).len(), 1);

    // a directory of another name is gone on with too
    let world = World::new("f73b", "fail");
    assert!(run(&world.spec(), &world.pinned()).outcome.is_err());
    std::fs::rename(
        world.root().join("v9.partial"),
        world.root().join("my-comfy"),
    )
    .unwrap();
    world.mode("ok");
    let ran = run(&world.spec(), &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert_eq!(entries(&world.root()), ["v9"]);
}

#[cfg(unix)]
#[test]
fn p3_f74_what_an_install_that_stopped_needs_is_counted_in_the_room_it_asks_for() {
    let world = World::new("f74", "fail");
    let mut spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    let written: u64 = list_unfinished(&world.root())[0].size_bytes;
    assert!(written > 0);

    // the disk has less than the document asks for, but more than is still to be written
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.space = Some(written + 1000);
    spec.free_space = |_| Some(1500);
    world.mode("ok");
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:?}", ran.outcome);

    // while an install that has written nothing is refused for the same disk
    let world = World::new("f74b", "ok");
    let mut spec = world.spec();
    spec.free_space = |_| Some(1500);
    let InstallKind::PythonApp(app) = &mut spec.kind else {
        unreachable!()
    };
    app.space = Some(3000);
    let error = run(&spec, &world.pinned()).outcome.unwrap_err();
    assert!(error.contains("not enough free space"), "{error}");
}

#[cfg(unix)]
#[test]
fn p3_f75_a_step_whose_files_are_gone_is_done_again_and_so_is_every_step_after_it() {
    // the environment was deleted by hand: it is made again, and the source and the pack are not fetched again
    let world = World::new("f75", "fail");
    let spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    let kept = world.root().join("v9.partial");
    world.mode("ok");
    std::fs::remove_dir_all(kept.join("venv")).unwrap();
    let (before, asked) = (lines(&world.log).len(), world.server.requests());
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    let after = lines(&world.log)[before..].to_vec();
    assert_eq!(
        after.iter().filter(|l| l.starts_with("venv ")).count(),
        1,
        "{after:#?}"
    );
    assert!(
        said(
            &ran,
            "Already done by the install that stopped: the source of v9"
        ) && said(&ran, "Making the Python environment"),
        "{:#?}",
        ran.said
    );
    assert_eq!(world.server.requests(), asked, "nothing was fetched again");

    // the source was deleted: it is fetched again, and everything made on top of it is made again
    let world = World::new("f75b", "fail");
    let spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    world.mode("ok");
    std::fs::remove_dir_all(world.root().join("v9.partial/app")).unwrap();
    let (before, asked) = (lines(&world.log).len(), world.server.requests());
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(
        said(&ran, "Downloading v9") && said(&ran, "Making the Python environment"),
        "{:#?}",
        ran.said
    );
    assert!(
        world.server.requests() > asked,
        "the source was fetched again"
    );
    assert!(
        lines(&world.log)[before..]
            .iter()
            .any(|l| l.starts_with("venv ")),
        "and what was made on top of it"
    );
    assert_eq!(entries(&world.root()), ["v9"]);
}

#[cfg(unix)]
#[test]
fn p3_f78_a_record_that_cannot_be_written_does_not_cost_the_work_of_an_install_that_stopped() {
    use std::os::unix::fs::PermissionsExt;
    // a program that runs with the rights of the superuser can write anywhere: the test has nothing to show
    // SAFETY: geteuid takes no arguments and cannot fail.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let world = World::new("f78", "fail");
    let spec = world.spec();
    assert!(run(&spec, &world.pinned()).outcome.is_err());
    let kept = world.root().join("v9.partial");
    world.mode("ok");

    // nothing can be written in the directory (a full disk says the same): the install says so, and goes
    // no further, and what was done is where it was
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o555)).unwrap();
    let ran = run(&spec, &world.pinned());
    std::fs::set_permissions(&kept, std::fs::Permissions::from_mode(0o755)).unwrap();
    let error = ran.outcome.unwrap_err();
    assert!(
        error.contains("could not write the record of the install"),
        "{error}"
    );
    assert!(
        kept.join("venv/bin/python").is_file(),
        "the environment is there"
    );
    let stopped = list_unfinished(&world.root());
    assert_eq!((stopped.len(), stopped[0].done), (1, 3), "{stopped:?}");

    // and the next install, with the cause gone, goes on from it
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(said(
        &ran,
        "Going on with the install that stopped (3 of 7 steps are done)"
    ));
}

// ---- the list of releases ---------------------------------------------------------------------------------------

mod feed {
    use super::*;
    use crate::prims::feed::{spawn_lookup, Build, BuildLookupEvent, LookupSpec};
    use crate::testkit::fake_github::FakeGithub;
    use quickai_core::plan::HostFacts;
    use serde_json::json;

    fn comfyui() -> quickai_core::registry::Engine {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        registry
            .engine("comfyui")
            .expect("the ComfyUI document")
            .clone()
    }

    fn release(tag: &str, prerelease: bool) -> serde_json::Value {
        json!({
            "tag_name": tag,
            "draft": false,
            "prerelease": prerelease,
            // what ComfyUI's releases carry: portable builds for Windows, and nothing for Linux
            "assets": [{
                "name": "ComfyUI_windows_portable_amd.7z",
                "browser_download_url": format!("https://github.com/Comfy-Org/ComfyUI/releases/download/{tag}/ComfyUI_windows_portable_amd.7z"),
                "size": 1_621_312_076u64,
                "digest": "sha256:19be1e3c8690c4f9eeefb0f2ee8f835a98e2fbf33cd9b80d0ca309f4c86d37c1",
            }],
        })
    }

    fn lookup(releases: Vec<serde_json::Value>, os: &str) -> (Vec<String>, Vec<Build>) {
        let fake = FakeGithub::with_releases(serde_json::Value::Array(releases));
        let spec = LookupSpec {
            engine: comfyui(),
            facts: HostFacts {
                os: os.into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
            api_base: fake.base(),
            token: None,
        };
        let rx = spawn_lookup(spec, || {});
        let (mut statuses, mut builds) = (Vec::new(), Vec::new());
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(BuildLookupEvent::Status(s)) => statuses.push(s),
                Ok(BuildLookupEvent::Found(b)) => builds.push(b),
                Ok(BuildLookupEvent::Done) => return (statuses, builds),
                _ => {}
            }
        }
        panic!("the look-up did not finish: {statuses:?}");
    }

    #[test]
    fn p3_f30_every_release_is_offered_as_its_source_whatever_its_files_and_the_tested_one_comes_first(
    ) {
        let (_, builds) = lookup(
            vec![
                release("v0.41.0", false),
                release("v0.40.0", false),
                release("v0.39.0", false),
                release("v0.39.0-rc1", true),
                release("v0.38.0", false),
            ],
            "linux",
        );
        // the tested release first, then the feed's order; the prerelease is left out, as the document says
        let tags: Vec<&str> = builds.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, ["v0.39.0", "v0.41.0", "v0.40.0", "v0.38.0"]);
        let tested = &builds[0];
        assert!(tested.tested && !builds[1].tested);
        assert_eq!(tested.label, "v0.39.0");
        assert_eq!(tested.asset_name, "v0.39.0.tar.gz");
        assert_eq!(
            tested.url,
            "https://github.com/Comfy-Org/ComfyUI/archive/refs/tags/v0.39.0.tar.gz"
        );
        let pin = tested
            .pin
            .as_deref()
            .expect("the tested release has its pin");
        assert_eq!(pin.len(), 64);
        assert!(
            builds[1].pin.is_none(),
            "an untested release has none, and says so when installed"
        );
        assert!(builds
            .iter()
            .all(|b| b.sha256.is_none() && b.overlays.is_empty()));
        // what an install of it is told: the source, its pin, no host digest
        let request = tested.install_request(&[]);
        assert_eq!(request.label, "v0.39.0");
        assert_eq!(request.base.url, tested.url);
        assert_eq!(request.base.pin.as_deref(), Some(pin));
        assert!(request.base.host_digest.is_none() && request.overlays.is_empty());
        // the same on every platform: the source is not a file for one of them
        let (_, on_windows) = lookup(vec![release("v0.39.0", false)], "windows");
        assert_eq!(on_windows.len(), 1);
    }

    #[test]
    fn p3_f31_the_tested_release_is_offered_when_the_first_page_of_the_feed_has_moved_past_it() {
        let (_, builds) = lookup(
            vec![release("v0.50.0", false), release("v0.49.0", false)],
            "linux",
        );
        let tags: Vec<&str> = builds.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, ["v0.39.0", "v0.50.0", "v0.49.0"]);
        assert!(builds[0].tested && builds[0].pin.is_some());
        // and with no release at all (a feed that says nothing), the tested one is still there to be installed
        let (_, none) = lookup(Vec::new(), "linux");
        assert_eq!(none.len(), 1);
        assert_eq!(none[0].tag, "v0.39.0");
    }
}

// ---- starting it and attaching to it ----------------------------------------------------------------------------

mod session {
    use super::*;
    use crate::events::ServerState;
    use crate::launch::{self, MachineFacts};
    use crate::runtime::session::Session;
    use crate::testkit::mini_http::MiniServer;
    use crate::testkit::{free_port, procs::kill_quietly};
    use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
    use quickai_core::plan::Selection;
    use quickai_core::registry::Registry;

    /// The sections of the ComfyUI document that start it, with test-sized timings.
    const DOC: &str = r#"schema = 1
kind = "engine"
id = "comfy_test"
requires = { primitives = ["spawn@1", "probe.http@1", "attach@1"] }

[engine]
name = "ComfyUI (test)"

[provides]
apis = []
default_port = 8188
needs_model = false

[locate]
strategies = [ { use = "setting", field = "bin_override" } ]

[launch]
strategies = ["attach_if_running", "spawn"]
strategy_field = "mode"

[launch.attach]
endpoint = "127.0.0.1:${public_port}"
identity = { path = "/system_stats", pointer = "/system/comfyui_version" }
liveness = { path = "/system_stats", every = "100ms", failures = 2 }

[launch.spawn]
program = "${runtime.binary}"
args = ["--port", "${listen_port}", "--base", "${field.work}"]
mkdir = ["${field.work}/models", "${field.work}/user/deep"]
group = true
stop = { policy = "graceful", grace = "300ms", exit_grace = "300ms" }

[launch.ready]
probe = { use = "http", path = "/system_stats", expect = 200 }
timeout = "5s"
interval = "25ms"

[[field]]
id = "mode"
type = "choice"
options = ["auto", "attach", "spawn"]
default = "auto"

[[field]]
id = "port"
type = "port"
default = 8188
bind.port = "public"

[[field]]
id = "work"
type = "dir"
default = "/nowhere"

[[field]]
id = "bin_override"
type = "runtime_path"
"#;

    fn registry() -> Registry {
        let source = MemorySource::new(&[("embedded:engines/comfy_test.toml", DOC)]);
        let registry = Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        registry
    }

    /// ComfyUI's page of facts, as far as QuickAI reads it.
    fn serve_stats(port: u16, version: Option<&str>) -> MiniServer {
        let body = match version {
            Some(v) => json!({"system": {"comfyui_version": v}, "devices": []}),
            None => json!({"detail": "not found"}),
        };
        for _ in 0..80 {
            let body = body.clone();
            if let Ok(server) = MiniServer::start(Some(port), move |req, resp| {
                if req.route() == "/system_stats" {
                    resp.json(200, &body, "application/json");
                } else {
                    resp.status(404).send(b"nope");
                }
            }) {
                return server;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
        panic!("could not bind a fake ComfyUI on port {port}");
    }
    use serde_json::json;

    fn settle(session: &mut Session, want: ServerState, log: &mut Vec<String>) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while Instant::now() < deadline {
            session.poll(&mut |line| log.push(line), &mut |_| {});
            if session.state() == want {
                return;
            }
            std::thread::sleep(Duration::from_millis(15));
        }
        panic!("never {want:?}, it is {:?}: {log:#?}", session.state());
    }

    struct World {
        dir: PathBuf,
        script: PathBuf,
        port: u16,
        registry: Registry,
    }

    impl World {
        fn new(name: &str) -> World {
            let dir = scratch(name).into_path();
            let script = write_script(
                &dir,
                "comfyui",
                &format!(
                    "echo \"$@\" > '{d}/args'\necho $$ > '{d}/pid'\nfor f in models user/deep; do ls -d \"$(echo \"$@\" | sed 's/.*--base //')\"/$f >> '{d}/folders' 2>&1; done\ntouch '{d}/started'\nsleep 60 &\nwait",
                    d = dir.display()
                ),
            );
            World {
                dir,
                script,
                port: free_port(),
                registry: registry(),
            }
        }

        fn plan(&self, mode: &str) -> quickai_core::plan::RunPlan {
            let selection = Selection::new("comfy_test")
                .set("comfy_test.port", self.port)
                .set("comfy_test.mode", mode)
                .set(
                    "comfy_test.work",
                    self.dir.join("work").display().to_string(),
                );
            launch::plan_selection(
                &self.registry,
                &selection,
                &self.script,
                MachineFacts::default(),
                None,
            )
            .unwrap_or_else(|e| panic!("{e:#?}"))
        }
    }

    impl Drop for World {
        fn drop(&mut self) {
            if let Ok(text) = std::fs::read_to_string(self.dir.join("pid")) {
                if let Ok(pid) = text.trim().parse() {
                    kill_quietly(pid);
                }
            }
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    #[test]
    fn p3_f40_comfyui_is_started_with_its_folders_made_and_no_model_and_stopped_gracefully() {
        let world = World::new("f40");
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&world.plan("auto"), "", || {});

        wait_until(Duration::from_secs(10), "the program to start", || {
            world.dir.join("started").exists()
        });
        // the folders it refuses when they are missing were made before it started, with their parents
        let ls = std::fs::read_to_string(world.dir.join("folders")).unwrap();
        assert!(!ls.contains("No such file"), "{ls}");
        assert!(
            world.dir.join("work/models").is_dir() && world.dir.join("work/user/deep").is_dir()
        );
        let args = std::fs::read_to_string(world.dir.join("args")).unwrap();
        assert!(
            args.trim()
                .starts_with(&format!("--port {} --base ", world.port)),
            "{args}"
        );

        // it answers as ComfyUI, and the session is running with no model loaded
        let _fake = serve_stats(world.port, Some("0.39.0"));
        settle(&mut session, ServerState::Running, &mut log);
        assert_eq!(session.model_name(), None);
        assert_eq!(session.port(), Some(world.port));
        assert!(!session.attached(), "QuickAI started it");

        session.stop();
        settle(&mut session, ServerState::Idle, &mut log);
        let pid: u32 = std::fs::read_to_string(world.dir.join("pid"))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        assert!(
            gone_within(pid, Duration::from_secs(5)),
            "the program was left running"
        );
    }

    /// The document of the tests, which also puts the application's own files in the folder it is given.
    fn registry_that_writes(write: &str) -> Registry {
        let doc = DOC.replace(
            "group = true",
            &format!("write = [ {write} ]\ngroup = true"),
        );
        let source = MemorySource::new(&[("embedded:engines/comfy_test.toml", &doc)]);
        let registry = Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        registry
    }

    #[test]
    fn p3_p18_the_files_of_the_application_are_put_before_the_program_starts_and_a_start_that_cannot_put_them_is_said_and_stops(
    ) {
        let mut world = World::new("p17");
        world.registry = registry_that_writes(
            r#"{ asset = "comfyui/extra_model_paths.yaml", to = "${field.work}/extra_model_paths.yaml" },
               { asset = "comfyui/quickai_bridge", to = "${field.work}/extensions/quickai_bridge" }"#,
        );
        // the program notes what it finds when it starts: the files are there before it is
        let noted = world.dir.join("noted");
        let work = world.dir.join("work");
        world.script = write_script(
            &world.dir,
            "comfyui",
            &format!(
                "ls '{w}/extra_model_paths.yaml' '{w}/extensions/quickai_bridge/__init__.py' '{w}/extensions/quickai_bridge/web/quickai_bridge.js' > '{n}' 2>&1\necho $$ > '{d}/pid'\ntouch '{d}/started'\nsleep 60 &\nwait",
                w = work.display(),
                n = noted.display(),
                d = world.dir.display()
            ),
        );
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&world.plan("auto"), "", || {});
        wait_until(Duration::from_secs(10), "the program to start", || {
            world.dir.join("started").exists()
        });
        let found = std::fs::read_to_string(&noted).unwrap();
        assert!(!found.contains("No such file"), "{found}");
        settle_log(&mut session, &mut log);
        assert!(
            log.iter().any(|l| l.contains("Put ")
                && l.contains("extensions")
                && l.contains("files written")),
            "the log says what was put: {log:#?}"
        );
        session.stop();
        std::thread::sleep(Duration::from_millis(300));
        session.poll(&mut |line| log.push(line), &mut |_| {});

        // the second start finds them as they were and writes nothing, and says nothing of it
        let first_lines = log.iter().filter(|l| l.starts_with("Put ")).count();
        std::fs::remove_file(world.dir.join("started")).unwrap();
        std::fs::write(
            work.join("extensions/quickai_bridge/fetch.py"),
            "an older one",
        )
        .unwrap();
        session.start(&world.plan("auto"), "", || {});
        wait_until(
            Duration::from_secs(10),
            "the program to start again",
            || world.dir.join("started").exists(),
        );
        settle_log(&mut session, &mut log);
        let puts: Vec<&String> = log.iter().filter(|l| l.starts_with("Put ")).collect();
        assert_eq!(
            puts.len(),
            first_lines + 1,
            "a file that differs is written again: {puts:#?}"
        );
        assert!(
            puts.last().unwrap().contains("(1 of 3 files written)"),
            "{puts:#?}"
        );
        assert_ne!(
            std::fs::read_to_string(work.join("extensions/quickai_bridge/fetch.py")).unwrap(),
            "an older one"
        );
        session.stop();

        // a place that cannot be written stops the start, and the program is not started without it
        let blocked = World::new("p17b");
        let mut blocked = blocked;
        blocked.registry = registry_that_writes(
            r#"{ asset = "comfyui/extra_model_paths.yaml", to = "${field.work}/models" }"#,
        );
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&blocked.plan("auto"), "", || {});
        settle(&mut session, ServerState::Crashed, &mut log);
        assert!(
            log.iter()
                .any(|l| l.starts_with("Could not put ") && l.contains("models")),
            "{log:#?}"
        );
        assert!(
            !blocked.dir.join("started").exists(),
            "no program without its files"
        );
    }

    /// Reads what the session has said so far, for a little while: it is said by another thread.
    fn settle_log(session: &mut Session, log: &mut Vec<String>) {
        let end = Instant::now() + Duration::from_millis(600);
        while Instant::now() < end {
            session.poll(&mut |line| log.push(line), &mut |_| {});
            std::thread::sleep(Duration::from_millis(20));
        }
    }

    #[test]
    fn p3_f41_a_comfyui_that_is_already_answering_is_attached_to_and_left_running() {
        let world = World::new("f41");
        let fake = serve_stats(world.port, Some("0.39.0"));
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&world.plan("auto"), "", || {});
        settle(&mut session, ServerState::Running, &mut log);
        assert!(session.attached(), "{log:#?}");
        assert!(
            !world.dir.join("started").exists(),
            "no second ComfyUI was started"
        );
        session.stop();
        settle(&mut session, ServerState::Idle, &mut log);
        // it was not ours to stop
        assert_eq!(
            crate::testkit::http::get(&format!("http://127.0.0.1:{}/system_stats", world.port))
                .status,
            200
        );
        drop(fake);
    }

    #[test]
    fn p3_f42_in_attach_mode_nothing_is_started_and_something_that_is_not_comfyui_is_not_attached_to(
    ) {
        let world = World::new("f42");
        // nothing answers: attach only has nowhere to go
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&world.plan("attach"), "", || {});
        settle(&mut session, ServerState::Crashed, &mut log);
        assert!(
            !world.dir.join("started").exists(),
            "attach mode never starts one"
        );

        // a web server that answers on the port is not taken for ComfyUI
        let other = World::new("f42b");
        let _fake = serve_stats(other.port, None);
        let mut session = Session::new();
        let mut log = Vec::new();
        session.start(&other.plan("attach"), "", || {});
        settle(&mut session, ServerState::Crashed, &mut log);
        assert!(!session.attached());
    }
}

// ---- the files of a model, each in its folder ---------------------------------------------------------------------

mod files {
    use super::*;
    use crate::bundles::{self, RECORD_SUFFIX};
    use crate::downloader::spawn_files;
    use crate::models::discover_in_sources;
    use quickai_core::lookup::present::{Download, Piece};
    use quickai_core::models::FileSource;

    fn piece(
        kind: Option<&str>,
        name: &str,
        bytes: &[u8],
        place: Option<&str>,
        url: String,
    ) -> Piece {
        Piece {
            kind: kind.map(str::to_string),
            label: kind.unwrap_or("model").to_string(),
            url,
            store_as: name.to_string(),
            name: name.to_string(),
            size: Some(bytes.len() as u64),
            sha256: Some(sha256_hex(bytes)),
            place: place.map(str::to_string),
        }
    }

    /// A model of three files in the three folders of ComfyUI, fetched from a server of its own.
    fn video(server: &FileServer, family: &str, vae: &[u8]) -> Download {
        let (weights, encoder) = (
            format!("{family}-q6.gguf").into_bytes(),
            b"T5 TEXT ENCODER".to_vec(),
        );
        server.add(&format!("/{family}.gguf"), weights.clone());
        server.add("/t5.gguf", encoder.clone());
        server.add("/vae.safetensors", vae.to_vec());
        Download {
            files: vec![
                piece(
                    None,
                    &format!("{family}-q6.gguf"),
                    &weights,
                    Some("diffusion_models"),
                    server.url(&format!("/{family}.gguf")),
                ),
                piece(
                    Some("text_encoder"),
                    "o--r--t5.gguf",
                    &encoder,
                    Some("text_encoders"),
                    server.url("/t5.gguf"),
                ),
                piece(
                    Some("vae"),
                    "o--r--vae.safetensors",
                    vae,
                    Some("vae"),
                    server.url("/vae.safetensors"),
                ),
            ],
            bundle: Some("video".into()),
            source: "huggingface:o/r".into(),
            mode: None,
        }
    }

    fn fetch(download: Download, models: &Path) -> Result<String, String> {
        spawn_files(download, models.to_path_buf(), || {})
            .wait(Duration::from_secs(30))
            .expect("the download finished")
    }

    fn sources(models: &Path) -> Vec<FileSource> {
        let source = |folder: &str, ext: &[&str]| FileSource {
            kind: "scan_dir".into(),
            path: models.join(folder).display().to_string(),
            ext: ext.iter().map(|e| e.to_string()).collect(),
            writable: true,
        };
        vec![
            source("diffusion_models", &[".gguf", ".safetensors"]),
            source("checkpoints", &[".safetensors", ".ckpt"]),
        ]
    }

    #[test]
    fn p3_f50_the_files_of_a_model_are_kept_in_the_folders_comfyui_looks_for_them_in() {
        let dir = scratch("f50");
        let server = FileServer::start();
        let models = dir.join("models");
        let weights = fetch(video(&server, "ltx", b"VAE"), &models).unwrap();
        assert_eq!(weights, "ltx-q6.gguf");

        // each file went to the folder of its kind, and nothing is left of the download
        assert!(models.join("diffusion_models/ltx-q6.gguf").is_file());
        assert!(models.join("text_encoders/o--r--t5.gguf").is_file());
        assert!(models.join("vae/o--r--vae.safetensors").is_file());
        for folder in ["diffusion_models", "text_encoders", "vae"] {
            let names = entries(&models.join(folder));
            assert!(
                names.iter().all(|n| !n.ends_with(".part")),
                "{folder}: {names:?}"
            );
        }
        assert_eq!(
            entries(&models),
            ["diffusion_models", "text_encoders", "vae"]
        );

        // the record is beside the weights and says where the others are
        let record = models
            .join("diffusion_models")
            .join(format!("ltx-q6.gguf{RECORD_SUFFIX}"));
        let text = std::fs::read_to_string(&record).unwrap();
        assert!(
            text.contains("\"dir\": \"text_encoders\"") && text.contains("\"dir\": \"vae\""),
            "{text}"
        );
        let weights_path = models.join("diffusion_models/ltx-q6.gguf");
        let companions = bundles::companions(&weights_path);
        let found: Vec<(String, PathBuf)> = companions
            .iter()
            .map(|c| (c.kind.clone(), c.path.clone()))
            .collect();
        assert_eq!(
            found,
            [
                (
                    "text_encoder".to_string(),
                    models.join("text_encoders/o--r--t5.gguf")
                ),
                ("vae".to_string(), models.join("vae/o--r--vae.safetensors")),
            ]
        );
        assert!(companions.iter().all(|c| c.exists()));

        // the picker lists the weights: not the record, and not the files of the other folders
        let listed = discover_in_sources(&sources(&models), &[]);
        assert_eq!(
            listed
                .iter()
                .map(|m| m.filename.as_str())
                .collect::<Vec<_>>(),
            ["ltx-q6.gguf"]
        );
        assert_eq!(listed[0].companions.len(), 2);
        assert!(
            listed[0].note.contains("With its text_encoder") || listed[0].note.contains("With its"),
            "{}",
            listed[0].note
        );
        // a model put in the checkpoints folder by hand is listed too, with its own ending
        std::fs::create_dir_all(models.join("checkpoints")).unwrap();
        std::fs::write(models.join("checkpoints/sdxl.safetensors"), b"x").unwrap();
        std::fs::write(models.join("checkpoints/notes.txt"), b"x").unwrap();
        let listed = discover_in_sources(&sources(&models), &[]);
        assert_eq!(
            listed
                .iter()
                .map(|m| m.filename.as_str())
                .collect::<Vec<_>>(),
            ["ltx-q6.gguf", "sdxl.safetensors"]
        );
    }

    #[test]
    fn p3_f51_deleting_a_model_deletes_its_files_in_every_folder_but_not_what_another_model_uses() {
        let dir = scratch("f51");
        let server = FileServer::start();
        let models = dir.join("models");
        fetch(video(&server, "a", b"SHARED VAE"), &models).unwrap();
        fetch(video(&server, "b", b"SHARED VAE"), &models).unwrap();
        // both models share the encoder and the VAE, which were fetched once
        assert_eq!(entries(&models.join("text_encoders")), ["o--r--t5.gguf"]);

        let first = bundles::remove_model(&models.join("diffusion_models/a-q6.gguf"));
        assert!(
            first.weights_error.is_none() && first.failed.is_empty(),
            "{first:?}"
        );
        assert!(first.removed.contains(&"a-q6.gguf".to_string()));
        assert!(first.removed.iter().any(|r| r.ends_with(RECORD_SUFFIX)));
        assert_eq!(
            first.kept,
            ["o--r--t5.gguf", "o--r--vae.safetensors"],
            "the other model still uses them"
        );
        assert!(
            models.join("text_encoders/o--r--t5.gguf").is_file()
                && models.join("vae/o--r--vae.safetensors").is_file()
        );
        assert!(!models.join("diffusion_models/a-q6.gguf").exists());

        let second = bundles::remove_model(&models.join("diffusion_models/b-q6.gguf"));
        assert!(second.kept.is_empty(), "{second:?}");
        assert_eq!(entries(&models.join("text_encoders")), Vec::<String>::new());
        assert_eq!(entries(&models.join("vae")), Vec::<String>::new());
        assert_eq!(
            entries(&models.join("diffusion_models")),
            Vec::<String>::new()
        );
    }

    #[test]
    fn p3_f52_a_record_is_not_believed_about_a_folder_that_is_not_a_plain_name() {
        let dir = scratch("f52");
        let folder = dir.join("models/diffusion_models");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(dir.join("models/vae")).unwrap();
        std::fs::write(folder.join("w.gguf"), b"w").unwrap();
        std::fs::write(dir.join("models/vae/v.safetensors"), b"v").unwrap();
        std::fs::write(dir.join("secret.txt"), b"s").unwrap();
        let record = |dir_value: &str, file: &str| {
            format!(
                r#"{{"version":1,"bundle":"video","source":"x","parts":[{{"kind":"vae","label":"VAE","file":"{file}","name":"v","dir":"{dir_value}"}}]}}"#
            )
        };
        let path = folder.join(format!("w.gguf{RECORD_SUFFIX}"));
        std::fs::write(&path, record("vae", "v.safetensors")).unwrap();
        assert_eq!(bundles::companions(&folder.join("w.gguf")).len(), 1);
        for evil in ["../..", "../vae", "vae/..", "/etc", "VAE", "a/b", ""] {
            std::fs::write(&path, record(evil, "v.safetensors")).unwrap();
            assert!(
                bundles::companions(&folder.join("w.gguf")).is_empty(),
                "a folder `{evil}` was believed"
            );
        }
        // nor a file name that is a path
        std::fs::write(&path, record("vae", "../../secret.txt")).unwrap();
        assert!(bundles::companions(&folder.join("w.gguf")).is_empty());
        // and deleting the model never touches what is outside the models folder
        let removed = bundles::remove_model(&folder.join("w.gguf"));
        assert!(removed.failed.is_empty(), "{removed:?}");
        assert!(dir.join("secret.txt").is_file());
    }

    #[test]
    fn p3_f53_a_folder_that_is_not_a_plain_name_is_refused_and_a_file_that_is_not_the_one_is_not_kept(
    ) {
        let dir = scratch("f53");
        let server = FileServer::start();
        let models = dir.join("models");
        // a piece that says to go outside the models folder fails, and writes nothing
        let mut download = video(&server, "ltx", b"VAE");
        download.files[1].place = Some("../escaped".into());
        let error = fetch(download, &models).unwrap_err();
        assert!(error.contains("is not the name of a folder"), "{error}");
        assert!(!dir.join("escaped").exists() && !models.join("../escaped").exists());

        // a file whose bytes are not the hash the list holds is thrown away, wherever its folder is
        let mut download = video(&server, "ltx", b"VAE");
        download.files[2].sha256 = Some("00".repeat(32));
        let error = fetch(download, &models).unwrap_err();
        assert!(error.contains("does not match the checksum"), "{error}");
        assert_eq!(
            entries(&models.join("vae")),
            Vec::<String>::new(),
            "not even a .part is left"
        );
        // the weights, which came first, stay: they run alone
        assert!(models.join("diffusion_models/ltx-q6.gguf").is_file());
    }
}

// ---- the real thing -------------------------------------------------------------------------------------------------

#[test]
#[ignore = "manual: needs the internet (cargo test -p quickai p3_f91 -- --ignored)"]
fn p3_f91_the_real_archives_still_have_the_digests_the_document_pins() {
    use quickai_core::acquire;
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry.engine("comfyui").expect("the ComfyUI document");
    let fetch = |url: &str| -> Vec<u8> {
        ureq::get(url)
            .header("User-Agent", "quickai-test")
            .call()
            .unwrap_or_else(|e| panic!("{url}: {e}"))
            .into_body()
            .with_config()
            .limit(64 * 1024 * 1024)
            .read_to_vec()
            .unwrap_or_else(|e| panic!("{url}: {e}"))
    };
    // the source of the tested release: GitHub makes the archive of a tag, and this is the one the pin is of
    let tag = acquire::tested_tag(engine).expect("a tested release");
    let url = acquire::source_archive_url(engine, tag).unwrap();
    let want = acquire::pin_for(engine, &acquire::source_archive_name(tag)).expect("a pin");
    assert_eq!(
        sha256_hex(&fetch(&url)),
        want,
        "{url} is not what it was when it was pinned"
    );
    // and each pack of nodes, which is pinned to a commit
    let quickai_core::doc::engine::Install::PythonApp { nodes, .. } = engine
        .doc
        .acquire
        .as_ref()
        .unwrap()
        .install
        .as_ref()
        .unwrap()
    else {
        panic!("a python_app install");
    };
    for node in nodes {
        assert_eq!(
            sha256_hex(&fetch(&node.url)),
            node.sha256.trim_start_matches("sha256:"),
            "{} is not what it was when it was pinned",
            node.url
        );
    }
}

#[test]
#[ignore = "manual: needs the internet, a Python 3.10 or newer, about 30 GB on a disk and the three files of LTX-Video 2B 0.9.8 distilled; QUICKAI_F90_DIR (a folder on a disk: /tmp may be memory) and QUICKAI_F90_MODELS (a models folder with diffusion_models, text_encoders and vae) say where; cargo test -p quickai p3_f90 -- --ignored --nocapture"]
fn p3_f90_the_real_comfyui_is_installed_started_and_makes_a_clip_by_the_applications_own_code() {
    use crate::events::ServerState;
    use crate::launch::{self, MachineFacts};
    use crate::runtime::session::Session;
    use crate::testkit::http;
    use quickai_core::acquire;
    use quickai_core::plan::Selection;

    let work = PathBuf::from(
        std::env::var("QUICKAI_F90_DIR").expect("QUICKAI_F90_DIR: a folder on a disk"),
    );
    let models = PathBuf::from(
        std::env::var("QUICKAI_F90_MODELS").expect("QUICKAI_F90_MODELS: a models folder"),
    );
    for folder in ["diffusion_models", "text_encoders", "vae"] {
        assert!(
            models.join(folder).is_dir(),
            "{} has no {folder}",
            models.display()
        );
    }
    std::fs::create_dir_all(&work).unwrap();

    // this machine, as the application sees it, with a data directory of this test's own
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry.engine("comfyui").expect("the ComfyUI document");
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    let mut facts = launch::host_facts(Path::new(""), machine, None);
    facts.data_dir = work.join("data").display().to_string();
    println!(
        "machine: {} vendors {:?}, targets {:?}",
        facts.os, facts.gpu_vendors, facts.gpu_targets
    );
    println!(
        "{}",
        acquire::install_summary(engine, &facts).unwrap_or_default()
    );

    // 1. the install: the tested release, as the runtime manager's button asks for it
    let spec = InstallSpec::from_engine(engine, &facts).expect("an install spec");
    let tag = acquire::tested_tag(engine).expect("a tested release");
    let request = InstallRequest {
        label: acquire::install_label(engine, tag, &acquire::source_archive_name(tag)),
        base: Source {
            url: acquire::source_archive_url(engine, tag).unwrap(),
            host_digest: None,
            pin: acquire::pin_for(engine, &acquire::source_archive_name(tag)),
            size: None,
        },
        overlays: Vec::new(),
    };
    let started = Instant::now();
    let launcher = install_blocking(&spec, &request, &|event| {
        if let JobEvent::Status(line) = event {
            println!("[install {:>4}s] {line}", started.elapsed().as_secs());
        }
    })
    .unwrap_or_else(|e| panic!("the install failed: {e}"));
    println!(
        "installed {} in {} s",
        launcher.display(),
        started.elapsed().as_secs()
    );
    assert!(launcher.is_file());

    // 2. started by the session, from the plan the document makes
    let port = crate::testkit::free_port();
    let selection = Selection::new("comfyui")
        .set("comfyui.port", port)
        .set("comfyui.models_dir", models.display().to_string());
    facts.binary = Some(launcher.display().to_string());
    let plan = quickai_core::plan::plan(&registry, &selection, &facts).expect("a plan");
    let mut session = Session::new();
    session.start(&plan, "", || {});
    let up = Instant::now();
    loop {
        session.poll(&mut |line| println!("[comfyui] {line}"), &mut |_| {});
        match session.state() {
            ServerState::Running => break,
            ServerState::Crashed | ServerState::Idle => panic!("ComfyUI did not start"),
            _ => {}
        }
        assert!(
            up.elapsed() < Duration::from_secs(300),
            "ComfyUI did not come up in 5 minutes"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    println!(
        "ComfyUI is up on port {port} after {} s",
        up.elapsed().as_secs()
    );
    let base = format!("http://127.0.0.1:{port}");
    let stats = http::get(&format!("{base}/system_stats")).json();
    println!("{}", stats["devices"]);
    assert!(stats["system"]["comfyui_version"].is_string());

    // 3. a workflow sent to it: the one of docs/examples/workflows, at the size the document's note says
    let mut workflow: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/examples/workflows/ltx-video-2b-t2v.json"
    ))
    .unwrap();
    // the loaders are given the files the models folder holds (the first of each folder): it may be the
    // folder the application's own downloads made, with the quantization the person chose
    let first_in = |folder: &str| -> String {
        let mut names: Vec<String> = std::fs::read_dir(models.join(folder))
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.ends_with(".gguf") || n.ends_with(".safetensors"))
            .collect();
        names.sort();
        names
            .into_iter()
            .next()
            .unwrap_or_else(|| panic!("no file in {folder}"))
    };
    workflow["1"]["inputs"]["unet_name"] = serde_json::json!(first_in("diffusion_models"));
    workflow["2"]["inputs"]["clip_name"] = serde_json::json!(first_in("text_encoders"));
    workflow["3"]["inputs"]["vae_name"] = serde_json::json!(first_in("vae"));
    workflow["11"]["inputs"]["noise_seed"] = serde_json::json!(20261008u64);
    workflow["13"]["inputs"]["filename_prefix"] = serde_json::json!("f90");
    let queued = http::post_json(
        &format!("{base}/prompt"),
        &serde_json::json!({ "prompt": workflow }),
    );
    assert_eq!(queued.status, 200, "{}", queued.body);
    let id = queued.json()["prompt_id"].as_str().unwrap().to_string();
    let made = Instant::now();
    let entry = loop {
        let history = http::get(&format!("{base}/history/{id}")).json();
        if let Some(entry) = history.get(&id) {
            break entry.clone();
        }
        session.poll(
            &mut |line| {
                if !line.contains("it/s") && !line.contains("s/it") {
                    println!("[comfyui] {line}")
                }
            },
            &mut |_| {},
        );
        assert!(
            made.elapsed() < Duration::from_secs(1500),
            "no clip in 25 minutes"
        );
        std::thread::sleep(Duration::from_secs(1));
    };
    println!(
        "the clip took {} s (the model was loaded for it)",
        made.elapsed().as_secs()
    );
    assert_eq!(
        entry["status"]["status_str"], "success",
        "{}",
        entry["status"]
    );
    let file = &entry["outputs"]["13"]["images"][0];
    let url = format!(
        "{base}/view?filename={}&subfolder={}&type={}",
        file["filename"].as_str().unwrap(),
        file["subfolder"].as_str().unwrap_or(""),
        file["type"].as_str().unwrap_or("output")
    );
    let bytes = ureq::get(&url)
        .call()
        .unwrap()
        .into_body()
        .with_config()
        .limit(256 * 1024 * 1024)
        .read_to_vec()
        .unwrap();
    let saved = work.join("f90.webm");
    std::fs::write(&saved, &bytes).unwrap();
    println!("saved {} ({} bytes)", saved.display(), bytes.len());
    assert!(
        bytes.len() > 50_000,
        "a clip of four seconds is more than that"
    );
    assert_eq!(
        &bytes[..4],
        [0x1a, 0x45, 0xdf, 0xa3],
        "a WebM file starts with the EBML header"
    );

    // 4. stopped, and nothing is left
    let pids: Vec<u32> = std::fs::read_dir("/proc")
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_string_lossy().parse::<u32>().ok())
                .filter(|pid| {
                    std::fs::read_to_string(format!("/proc/{pid}/cmdline"))
                        .is_ok_and(|c| c.contains(&format!("--port\0{port}\0")))
                })
                .collect()
        })
        .unwrap_or_default();
    session.stop();
    let stopping = Instant::now();
    while session.is_active() && stopping.elapsed() < Duration::from_secs(30) {
        session.poll(&mut |line| println!("[comfyui] {line}"), &mut |_| {});
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(!session.is_active(), "ComfyUI did not stop");
    for pid in pids {
        assert!(
            gone_within(pid, Duration::from_secs(10)),
            "process {pid} was left running"
        );
    }
    println!("stopped in {} ms", stopping.elapsed().as_millis());
}

#[test]
#[ignore = "manual: needs a running ComfyUI whose models folder has the three files a look-up fetches for Wan 2.2 5B Turbo (the model, the text encoder, the VAE) and a card that runs it (183 s on a 12 GB RX 6700 XT); QUICKAI_Q15_COMFY=http://127.0.0.1:PORT; cargo test -p quickai p3_q15 -- --ignored --nocapture"]
fn p3_q15_the_example_workflow_makes_a_clip_from_the_three_files_a_look_up_fetches() {
    use crate::testkit::http;
    use serde_json::json;

    let base = std::env::var("QUICKAI_Q15_COMFY")
        .expect("QUICKAI_Q15_COMFY: the address of a running ComfyUI");
    let workflow: serde_json::Value = serde_json::from_str(include_str!(
        "../../../../docs/examples/workflows/wan22-ti2v-5b-turbo-t2v.json"
    ))
    .unwrap();

    // the loaders of that ComfyUI list the three files, each under the name the workflow loads
    for (node, kind, input) in [
        ("UnetLoaderGGUF", "1", "unet_name"),
        ("CLIPLoaderGGUF", "2", "clip_name"),
        ("VAELoader", "3", "vae_name"),
    ] {
        let info = http::get(&format!("{base}/object_info/{node}")).json();
        let wanted = workflow[kind]["inputs"][input].as_str().unwrap();
        let listed = info[node]["input"]["required"][input][0].to_string();
        assert!(
            listed.contains(wanted),
            "{node} does not list {wanted}: {listed}"
        );
    }

    // it is queued, and ends well, with a video
    let reply = http::post_json(
        &format!("{base}/prompt"),
        &json!({"prompt": workflow, "client_id": "quickai-q15"}),
    );
    assert_eq!(reply.status, 200, "{}", reply.body);
    let id = reply.json()["prompt_id"].as_str().unwrap().to_string();
    let started = Instant::now();
    let entry = loop {
        assert!(
            started.elapsed() < Duration::from_secs(1200),
            "no result after 20 minutes"
        );
        let history = http::get(&format!("{base}/history/{id}")).json();
        if let Some(entry) = history.get(&id) {
            break entry.clone();
        }
        std::thread::sleep(Duration::from_secs(3));
    };
    println!(
        "ended after {} s: {}",
        started.elapsed().as_secs(),
        entry["status"]
    );
    assert_eq!(
        entry["status"]["status_str"], "success",
        "{}",
        entry["status"]
    );
    let file = entry["outputs"]["10"]["images"][0]["filename"]
        .as_str()
        .unwrap_or_else(|| panic!("no clip in {}", entry["outputs"]));
    assert!(file.ends_with(".webm"), "{file}");
    println!("the clip is {file} in the output folder of that ComfyUI");
}
