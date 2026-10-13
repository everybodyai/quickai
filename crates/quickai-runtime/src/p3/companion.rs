//! Group T of the test plan (work package 4.16): a program that goes with an engine (Open WebUI beside
//! Ollama), from the package index to the window. The document and its plan are checked in
//! `quickai-core/tests/p3_companion.rs`; what is here needs processes, servers and a window:
//!
//! * the package index is a file server that answers the JSON a real one does (the shape was read from
//!   `https://pypi.org/pypi/open-webui/json`), and the wheel is a file it serves;
//! * the Python is the shell script of the ComfyUI tests, which also leaves in the environment the program
//!   a wheel puts there, and writes down which Python ran pip, so that "nothing is installed into a Python
//!   the person has" is read from what happened;
//! * the program that goes with the engine is a shell script that records how it was started (its
//!   arguments, its working directory, its environment), and the page it serves is a fake that answers
//!   `/health`.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use quickai_core::doc::engine::Verify;

use super::comfy::{entries, fake_python, lines, run, said};
use crate::prims::install::{
    install_blocking, list_installed, list_unfinished, InstallKind, InstallRequest, InstallSpec,
    PipPlan, PythonApp, Runs, Smoke, Source, CANCELLED,
};
use crate::testkit::archives::sha256_hex;
use crate::testkit::files::FileServer;
use crate::testkit::scratch;

/// The sha256 of the wheel of the release the document was tested with, as the document pins it.
fn real_pin() -> String {
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry
        .engine("openwebui")
        .expect("the Open WebUI document");
    quickai_core::acquire::pin_for(engine, "open_webui-0.11.4-py3-none-any.whl")
        .expect("the tested wheel is pinned")
}

// ---- the list of releases on the index ----------------------------------------------------------------------------

mod feed {
    use super::*;
    use crate::prims::feed::{spawn_lookup, Build, BuildLookupEvent, LookupSpec};
    use quickai_core::doc::engine::Feed;
    use quickai_core::plan::HostFacts;
    use serde_json::{json, Value};

    fn open_webui() -> quickai_core::registry::Engine {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        registry
            .engine("openwebui")
            .expect("the Open WebUI document")
            .clone()
    }

    const WHEEL: &str = "py3-none-any.whl";

    /// A file of a release, in the shape the index lists it.
    fn file(version: &str, kind: &str, upload: &str, sha: &str) -> Value {
        let (name, packagetype) = match kind {
            "pure" => (format!("open_webui-{version}-{WHEEL}"), "bdist_wheel"),
            "native" => (
                format!("open_webui-{version}-cp312-cp312-manylinux_2_28_x86_64.whl"),
                "bdist_wheel",
            ),
            _ => (format!("open_webui-{version}.tar.gz"), "sdist"),
        };
        json!({
            "filename": name,
            "url": format!("https://files.pythonhosted.org/packages/ab/cd/ef/{name}"),
            "size": 98_894_348u64,
            "packagetype": packagetype,
            "yanked": kind == "yanked",
            "upload_time_iso_8601": upload,
            "digests": { "sha256": sha, "md5": "0" },
        })
    }

    fn release(version: &str, kind: &str, upload: &str) -> (String, Vec<Value>) {
        let sha = format!("{:0>64}", version.replace('.', ""));
        let kind_of_file = if kind == "yanked" { "pure" } else { kind };
        let mut f = file(version, kind_of_file, upload, &sha);
        if kind == "yanked" {
            f["yanked"] = json!(true);
        }
        (version.to_string(), vec![f])
    }

    fn index(releases: Vec<(String, Vec<Value>)>) -> String {
        let map: serde_json::Map<String, Value> = releases
            .into_iter()
            .map(|(v, files)| (v, Value::Array(files)))
            .collect();
        json!({ "info": { "name": "open-webui" }, "releases": map }).to_string()
    }

    struct Found {
        statuses: Vec<String>,
        builds: Vec<Build>,
        failed: bool,
    }

    fn lookup(server: &FileServer) -> Found {
        lookup_of(open_webui(), server)
    }

    fn lookup_of(engine: quickai_core::registry::Engine, server: &FileServer) -> Found {
        let spec = LookupSpec {
            engine,
            facts: HostFacts {
                os: "linux".into(),
                arch: "x86_64".into(),
                ..Default::default()
            },
            api_base: server.base().to_string(),
            token: None,
        };
        let rx = spawn_lookup(spec, || {});
        let (mut statuses, mut builds, mut failed) = (Vec::new(), Vec::new(), false);
        let deadline = Instant::now() + Duration::from_secs(30);
        while Instant::now() < deadline {
            match rx.recv_timeout(Duration::from_millis(200)) {
                Ok(BuildLookupEvent::Status(s)) => statuses.push(s),
                Ok(BuildLookupEvent::Found(b)) => builds.push(b),
                Ok(BuildLookupEvent::Failed) => failed = true,
                Ok(BuildLookupEvent::Done) => {
                    return Found {
                        statuses,
                        builds,
                        failed,
                    }
                }
                _ => {}
            }
        }
        panic!("the look-up did not finish: {statuses:?}");
    }

    #[test]
    fn p3_t20_the_index_lists_the_newest_releases_that_have_a_wheel_the_tested_one_first() {
        let pin = real_pin();
        let mut tested = release("0.11.4", "pure", "2026-09-01T10:00:00.000000Z");
        tested.1[0]["digests"]["sha256"] = json!(pin.to_uppercase());
        let server = FileServer::start();
        server.add(
            "/pypi/open-webui/json",
            index(vec![
                release("0.12.0", "pure", "2026-10-09T10:00:00.000000Z"),
                // a pre-release is not offered unless the document says so
                release("0.12.0rc1", "pure", "2026-10-01T10:00:00.000000Z"),
                // a release that has only its source cannot be installed from a wheel
                release("0.11.9", "sdist", "2026-09-30T10:00:00.000000Z"),
                // one that was withdrawn is not offered
                release("0.11.8", "yanked", "2026-09-28T10:00:00.000000Z"),
                // a wheel for one platform is not the one for every platform
                release("0.11.7", "native", "2026-09-25T10:00:00.000000Z"),
                release("0.11.6", "pure", "2026-09-20T10:00:00.000000Z"),
                release("0.11.5", "pure", "2026-09-10T10:00:00.000000Z"),
                tested,
                release("0.11.3", "pure", "2026-08-20T10:00:00.000000Z"),
                release("0.11.2", "pure", "2026-08-10T10:00:00.000000Z"),
            ])
            .into_bytes(),
        );
        let found = lookup(&server);
        let tags: Vec<&str> = found.builds.iter().map(|b| b.tag.as_str()).collect();
        // the tested release first, then the newest; five of them, as the document says
        assert_eq!(tags, ["0.11.4", "0.12.0", "0.11.6", "0.11.5", "0.11.3"]);
        assert!(!found.failed);
        assert!(
            found.statuses[0].contains("pypi.org/project/open-webui"),
            "{:?}",
            found.statuses
        );
        let tested = &found.builds[0];
        assert!(tested.tested && found.builds[1..].iter().all(|b| !b.tested));
        assert_eq!(tested.label, "0.11.4", "a build is called by its release");
        assert_eq!(tested.asset_name, "open_webui-0.11.4-py3-none-any.whl");
        assert_eq!(
            tested.url,
            "https://files.pythonhosted.org/packages/ab/cd/ef/open_webui-0.11.4-py3-none-any.whl",
            "the address is the index's own"
        );
        assert_eq!(tested.size_bytes, Some(98_894_348));
        // the digest the index lists is the file's, and the document's pin is for the tested release alone
        assert_eq!(
            tested.sha256.as_deref(),
            Some(pin.as_str()),
            "bare, lower case"
        );
        assert_eq!(tested.pin.as_deref(), Some(pin.as_str()));
        assert!(!tested.conflict);
        for other in &found.builds[1..] {
            assert!(
                other.pin.is_none() && other.sha256.is_some(),
                "{}",
                other.tag
            );
        }
        // and what an install of it is told
        let request = tested.install_request(&[]);
        assert_eq!(request.label, "0.11.4");
        assert_eq!(request.base.url, tested.url);
        assert_eq!(request.base.pin.as_deref(), Some(pin.as_str()));
        assert_eq!(request.base.host_digest.as_deref(), Some(pin.as_str()));
        assert!(request.overlays.is_empty());
    }

    #[test]
    fn p3_t21_the_tested_release_is_asked_for_by_itself_when_the_list_has_moved_past_it() {
        let server = FileServer::start();
        let newer: Vec<(String, Vec<Value>)> = (5..13)
            .map(|n| {
                release(
                    &format!("0.12.{n}"),
                    "pure",
                    &format!("2026-10-{n:02}T10:00:00Z"),
                )
            })
            .collect();
        server.add("/pypi/open-webui/json", index(newer).into_bytes());
        let one =
            json!({ "urls": [ file("0.11.4", "pure", "2026-09-01T10:00:00Z", &real_pin()) ] });
        server.add("/pypi/open-webui/0.11.4/json", one.to_string().into_bytes());
        let found = lookup(&server);
        let tags: Vec<&str> = found.builds.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(tags, ["0.11.4", "0.12.12", "0.12.11", "0.12.10", "0.12.9"]);
        assert!(found.builds[0].tested && found.builds[0].pin.is_some());
        assert!(
            server
                .paths_asked()
                .contains(&"/pypi/open-webui/0.11.4/json".to_string()),
            "{:?}",
            server.paths_asked()
        );
    }

    #[test]
    fn p3_t22_a_pin_that_the_index_does_not_agree_with_means_the_file_changed_after_it_was_tested()
    {
        let server = FileServer::start();
        let mut changed = release("0.11.4", "pure", "2026-09-01T10:00:00Z");
        changed.1[0]["digests"]["sha256"] = json!("f".repeat(64));
        server.add("/pypi/open-webui/json", index(vec![changed]).into_bytes());
        let found = lookup(&server);
        assert_eq!(found.builds.len(), 1);
        let build = &found.builds[0];
        assert!(build.tested && build.conflict, "{build:?}");
        assert_eq!(build.sha256.as_deref(), Some("f".repeat(64).as_str()));
        assert_eq!(build.pin.as_deref(), Some(real_pin().as_str()));
    }

    #[test]
    fn p3_t23_an_index_that_does_not_answer_is_said_and_one_with_no_wheel_offers_nothing() {
        // nothing is served: the index says 404
        let server = FileServer::start();
        let found = lookup(&server);
        assert!(found.failed && found.builds.is_empty());
        assert!(
            found
                .statuses
                .iter()
                .any(|s| s.starts_with("Lookup failed:")),
            "{:?}",
            found.statuses
        );
        // a package with releases and no wheel among them: nothing to offer, and nothing is wrong
        let server = FileServer::start();
        server.add(
            "/pypi/open-webui/json",
            index(vec![release("0.12.0", "sdist", "2026-10-09T10:00:00Z")]).into_bytes(),
        );
        let found = lookup(&server);
        assert!(
            !found.failed && found.builds.is_empty(),
            "{:?}",
            found.builds
        );
    }
    /// What the real index answered (`tests/fixtures/pypi/`, made by `record.py` from `https://pypi.org/pypi/open-webui/json` and
    /// `.../0.11.4/json`, 2026-10-10: the files as the index gave them, 15 of its 166 releases).
    fn recorded(name: &str) -> Vec<u8> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/pypi")
            .join(name);
        std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
    }

    #[test]
    fn p3_t23b_what_the_real_index_answered_is_read_as_it_was_meant() {
        let server = FileServer::start();
        server.add("/pypi/open-webui/json", recorded("open-webui.json"));
        server.add(
            "/pypi/open-webui/0.11.4/json",
            recorded("open-webui-0.11.4.json"),
        );
        // the five the document shows: the tested release first and then the newest, by when they were uploaded (the index
        // lists its releases in the order of their names, so `0.9.6` comes after `0.11.4` in what it sends and is older)
        let found = lookup(&server);
        let tags: Vec<&str> = found.builds.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(
            tags,
            ["0.11.4", "0.11.3", "0.11.2", "0.11.1", "0.11.0"],
            "{:?}",
            found.statuses
        );
        let tested = &found.builds[0];
        assert!(tested.tested && !tested.conflict, "{tested:?}");
        assert_eq!(
            tested.sha256.as_deref(),
            Some(real_pin().as_str()),
            "the digest the index lists is what the document pins"
        );
        assert_eq!(tested.size_bytes, Some(98_894_348));
        assert_eq!(tested.asset_name, "open_webui-0.11.4-py3-none-any.whl");
        assert!(
            tested
                .url
                .starts_with("https://files.pythonhosted.org/packages/")
                && tested.url.ends_with("/open_webui-0.11.4-py3-none-any.whl"),
            "{}",
            tested.url
        );
        // everything asked for: the twelve releases that are not pre-releases, newest first, and none of the `.devN` ones
        let mut engine = open_webui();
        if let Some(Feed::Pypi { show, .. }) = engine
            .doc
            .acquire
            .as_mut()
            .and_then(|acquire| acquire.feed.as_mut())
        {
            *show = 100;
        }
        let found = lookup_of(engine, &server);
        let tags: Vec<&str> = found.builds.iter().map(|b| b.tag.as_str()).collect();
        assert_eq!(
            tags,
            [
                "0.11.4", "0.11.3", "0.11.2", "0.11.1", "0.11.0", "0.10.2", "0.10.1", "0.10.0",
                "0.9.6", "0.9.5", "0.9.4", "0.9.3"
            ]
        );
    }
}

// ---- installing a wheel ---------------------------------------------------------------------------------------

/// What an install of a wheel is run with: the fake Python, a file server with the wheel in it, and a root
/// of builds of their own.
struct World {
    server: FileServer,
    dir: PathBuf,
    python: PathBuf,
    log: PathBuf,
    wheel: Vec<u8>,
}

const WHEEL_NAME: &str = "open_webui-9.9.9-py3-none-any.whl";

impl World {
    fn new(name: &str, mode: &str) -> World {
        let dir = scratch(name).into_path();
        let (python, log) = fake_python(&dir, "3 98", mode);
        let server = FileServer::start();
        // a wheel is a zip; the fake pip does not read it, but the installer does check its bytes
        let wheel = b"PK\x03\x04 a wheel, for the purposes of a test\n".to_vec();
        server.add(&format!("/{WHEEL_NAME}"), wheel.clone());
        World {
            server,
            dir,
            python,
            log,
            wheel,
        }
    }

    fn root(&self) -> PathBuf {
        self.dir.join("openwebui")
    }

    fn mode(&self, mode: &str) {
        std::fs::write(format!("{}.mode", self.log.display()), mode).unwrap();
    }

    fn spec(&self) -> InstallSpec {
        InstallSpec {
            root: self.root(),
            binary: "open-webui".into(),
            search_depth: 0,
            chmod: true,
            fallback_label: "open-webui-build".into(),
            verify: Verify::Digest,
            smoke: Some(Smoke {
                args: vec!["--help".into()],
                timeout: Duration::from_secs(20),
            }),
            kind: InstallKind::PythonApp(Box::new(PythonApp {
                runs: Runs::Program("open-webui".into()),
                versions: vec!["3.98".into()],
                python: Some(self.python.clone()),
                pip: vec![
                    PipPlan {
                        id: "torch_cpu".into(),
                        label: "PyTorch for the processor".into(),
                        packages: vec!["torch".into()],
                        requirements: None,
                        wheel: false,
                        index: Some("https://example.invalid/whl/cpu".into()),
                        weight: 2,
                    },
                    PipPlan {
                        id: "open_webui".into(),
                        label: "Open WebUI and what it needs".into(),
                        packages: Vec::new(),
                        requirements: None,
                        wheel: true,
                        index: None,
                        weight: 6,
                    },
                ],
                nodes: Vec::new(),
                space: None,
            })),
            ..Default::default()
        }
    }

    fn request(&self, pin: Option<String>, host_digest: Option<String>) -> InstallRequest {
        InstallRequest {
            label: "9.9.9".into(),
            base: Source {
                url: self.server.url(&format!("/{WHEEL_NAME}")),
                host_digest,
                pin,
                size: None,
            },
            overlays: Vec::new(),
        }
    }

    fn pinned(&self) -> InstallRequest {
        self.request(Some(format!("sha256:{}", sha256_hex(&self.wheel))), None)
    }
}

impl Drop for World {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

#[test]
fn p3_t24_a_wheel_is_installed_into_an_environment_of_its_own_and_the_launcher_runs_its_program() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new("t24", "ok");
    let spec = world.spec();
    let ran = run(&spec, &world.pinned());
    let launcher = ran
        .outcome
        .clone()
        .unwrap_or_else(|e| panic!("{e}\n{:#?}", ran.said));
    let root = world.root();
    assert_eq!(launcher, root.join("9.9.9").join("open-webui"));
    // the build is whole, and nothing of the install is left beside it
    assert_eq!(entries(&root), ["9.9.9"]);
    let build = root.join("9.9.9");
    assert_eq!(
        entries(&build),
        ["open-webui", "venv"],
        "the launcher and the environment: no source, no copy of the wheel, no temporary files"
    );
    assert!(build.join("venv/bin/python").is_file() && build.join("venv/pyvenv.cfg").is_file());
    assert_ne!(
        std::fs::metadata(&launcher).unwrap().permissions().mode() & 0o111,
        0,
        "the launcher can be run"
    );

    // the launcher runs the environment's own Python and tells it what the program of the wheel calls (the
    // package's records say `open_webui:app`), with the person's own Python variables put away first. It does
    // not run the script of the environment: that holds the path of the folder it was made in, which is not
    // where the build is once it is whole (found by running the real thing: `exec: .../venv/bin/open-webui:
    // not found`)
    let text = std::fs::read_to_string(&launcher).unwrap();
    assert!(
        text.starts_with("#!/bin/sh")
            && text.contains(
                r#"exec "$here/venv/bin/python" -c "import sys; from open_webui import app; sys.argv[0] = 'open-webui'; sys.exit(app())" "$@""#
            )
            && text
                .contains("unset PYTHONPATH PYTHONHOME PYTHONSTARTUP PYTHONUSERBASE VIRTUAL_ENV")
            && text.contains("PYTHONNOUSERSITE=1")
            && !text.contains("venv/bin/open-webui"),
        "{text}"
    );

    // what it asked of the Python, in order: an environment, PyTorch from its index alone, the wheel that
    // was checked (by its path, in the folder of the install), what the program of the wheel calls, and the check
    let log = lines(&world.log);
    let staging = format!("{}/9.9.9.partial", root.display());
    assert_eq!(log.len(), 5, "{log:#?}");
    assert_eq!(log[0], format!("venv {staging}/venv"));
    assert!(
        log[1].starts_with(&format!("pip TMPDIR={staging}/tmp install --no-input"))
            && log[1].contains("--only-binary=:all:")
            && log[1].contains("--index-url https://example.invalid/whl/cpu torch")
            && log[1].ends_with("--no-cache-dir"),
        "{}",
        log[1]
    );
    assert!(
        log[2].contains(&format!("{staging}/wheel/{WHEEL_NAME}"))
            && !log[2].contains("--index-url")
            && log[2].contains("--only-binary=:all:")
            && log[2].ends_with("--no-cache-dir"),
        "only wheels for what the wheel needs, and no copy of it in a cache: {}",
        log[2]
    );
    assert_eq!(
        log[3], "entry open-webui",
        "the environment is asked what the program calls"
    );
    assert_eq!(
        log[4], "run open-webui --help",
        "the check runs the program"
    );
    // and the launcher still runs where the build was moved to, which the check, made before the move, cannot say
    let moved = std::process::Command::new(&launcher)
        .arg("--version")
        .output()
        .expect("the launcher runs");
    assert!(moved.status.success(), "{moved:?}");
    assert_eq!(
        lines(&world.log).last().map(String::as_str),
        Some("run open-webui --version"),
        "the program was run by the launcher of the moved build"
    );
    // the room was the install's own: the build is found as one
    let builds = list_installed(&spec);
    assert_eq!(builds.len(), 1);
    assert_eq!(builds[0].binary_path, launcher);
    assert!(said(&ran, "Checked 9.9.9 against its sha256"));
}

#[test]
fn p3_t25_a_wheel_that_is_not_the_one_tested_is_refused_before_pip_and_an_unpinned_one_says_so() {
    let world = World::new("t25", "ok");
    let spec = world.spec();
    // the file is not the one that was tested: refused, and pip is never asked
    let wrong = world.request(Some(format!("sha256:{}", "0".repeat(64))), None);
    let ran = run(&spec, &wrong);
    let error = ran.outcome.clone().unwrap_err();
    assert!(error.contains("does not match its checksum"), "{error}");
    assert!(lines(&world.log).is_empty(), "{:?}", lines(&world.log));
    assert!(
        !world.root().join("9.9.9").exists() && list_unfinished(&world.root()).is_empty(),
        "nothing is left of it"
    );
    // a wheel the index vouches for and the document does not know is installed, and the log says it was
    // not checked by the document
    let ran = run(&spec, &world.request(None, Some(sha256_hex(&world.wheel))));
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(
        said(&ran, "Checked 9.9.9 against its sha256"),
        "{:#?}",
        ran.said
    );
    // and one with neither says so
    let world = World::new("t25b", "ok");
    let ran = run(&world.spec(), &world.request(None, None));
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(said(&ran, "is not verified"), "{:#?}", ran.said);
}

#[test]
fn p3_t26_an_address_that_is_not_a_wheel_is_refused_before_anything_is_fetched() {
    let world = World::new("t26", "ok");
    for url in [
        world.server.url("/open_webui-9.9.9.tar.gz"),
        world.server.url("/not-a-wheel"),
        world.server.url("/.whl"),
    ] {
        let request = InstallRequest::bare(&url, "9.9.9");
        let error = run(&world.spec(), &request).outcome.unwrap_err();
        assert!(
            error.contains("is not the address of a wheel"),
            "{url}: {error}"
        );
    }
    assert_eq!(
        world.server.requests(),
        0,
        "nothing was asked of the network"
    );
    assert!(lines(&world.log).is_empty());
}

#[test]
fn p3_t27_an_install_that_stopped_after_its_packages_keeps_the_wheel_and_goes_on_without_fetching_it_again(
) {
    let world = World::new("t27", "smoke_fail");
    let spec = world.spec();
    let ran = run(&spec, &world.pinned());
    let error = ran.outcome.clone().unwrap_err();
    assert!(error.contains("ImportError"), "{error}");
    // the work is kept: the environment, the packages, and the wheel they were made from
    let kept = world.root().join("9.9.9.partial");
    assert!(kept.join("venv/bin/python").is_file());
    assert!(
        kept.join("wheel").join(WHEEL_NAME).is_file(),
        "the wheel is kept until the build is whole: a step that is done must not be undone for want of its file"
    );
    let stopped = list_unfinished(&world.root());
    assert_eq!(stopped.len(), 1, "{stopped:?}");
    let source = stopped[0]
        .source
        .clone()
        .expect("the record says where the file came from");
    assert_eq!(source.url, world.server.url(&format!("/{WHEEL_NAME}")));
    assert_eq!(
        source.pin.as_deref(),
        Some(format!("sha256:{}", sha256_hex(&world.wheel)).as_str())
    );
    let fetched_before = world.server.requests();

    // with the cause gone it goes on: no wheel is fetched, no environment is made, no pip is run again
    world.mode("ok");
    let ran = run(&spec, &world.pinned());
    assert!(ran.outcome.is_ok(), "{:#?}", ran.said);
    assert!(
        said(&ran, "Going on with the install that stopped"),
        "{:#?}",
        ran.said
    );
    assert_eq!(
        world.server.requests(),
        fetched_before,
        "nothing was fetched again"
    );
    let log = lines(&world.log);
    assert_eq!(
        log.iter().filter(|l| l.starts_with("venv ")).count(),
        1,
        "the environment was made once: {log:#?}"
    );
    assert_eq!(
        log.iter().filter(|l| l.starts_with("pip ")).count(),
        2,
        "each pip step was run once: {log:#?}"
    );
    // and the whole build has nothing of the install in it
    assert_eq!(entries(&world.root().join("9.9.9")), ["open-webui", "venv"]);
}

#[test]
fn p3_t28_each_application_has_an_environment_of_its_own_and_nothing_is_installed_into_the_python_that_made_it(
) {
    // two applications installed side by side, as Open WebUI and ComfyUI are: each is made in the folder of
    // its own build, the Python that is found is used for `-m venv` and for nothing else, and pip is run by
    // the environment's own Python, so that nothing is installed into a Python the person has
    let a = World::new("t28a", "ok");
    let b = World::new("t28b", "ok");
    assert!(run(&a.spec(), &a.pinned()).outcome.is_ok());
    assert!(run(&b.spec(), &b.pinned()).outcome.is_ok());
    let (venv_a, venv_b) = (a.root().join("9.9.9/venv"), b.root().join("9.9.9/venv"));
    assert_ne!(venv_a, venv_b);
    for (world, venv) in [(&a, &venv_a), (&b, &venv_b)] {
        assert!(venv.join("bin/python").is_file());
        let who = std::fs::read_to_string(format!("{}.who", world.log.display())).unwrap();
        let ran_pip: Vec<&str> = who.lines().collect();
        assert_eq!(ran_pip.len(), 2, "{ran_pip:?}");
        for python in ran_pip {
            assert!(
                python.contains("/9.9.9.partial/venv/bin/python"),
                "pip was run by {python}, which is not the environment of the build"
            );
            assert_ne!(
                Path::new(python),
                world.python,
                "pip was run by the Python that was found"
            );
        }
    }
    // deleting one build leaves the other whole
    crate::prims::install::delete_installed(&a.root().join("9.9.9")).unwrap();
    assert!(!venv_a.exists() && venv_b.join("bin/python").is_file());
}

#[test]
fn p3_t29_an_install_that_is_cancelled_leaves_nothing_and_no_wheel() {
    let world = World::new("t29", "hang");
    let spec = world.spec();
    let cancel = spec.cancel.clone();
    let root = world.root();
    let watcher = std::thread::spawn(move || {
        // when pip is under way (the environment is there), the person presses Cancel
        let deadline = Instant::now() + Duration::from_secs(20);
        while Instant::now() < deadline {
            if root.join("9.9.9.partial/venv/bin/python").is_file() {
                std::thread::sleep(Duration::from_millis(300));
                cancel.store(true, std::sync::atomic::Ordering::SeqCst);
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
    });
    let events = std::sync::Mutex::new(Vec::new());
    let outcome = install_blocking(&spec, &world.pinned(), &|event| {
        if let crate::jobs::JobEvent::Status(line) = event {
            events.lock().unwrap().push(line);
        }
    });
    watcher.join().unwrap();
    assert_eq!(outcome, Err(CANCELLED.to_string()));
    assert!(
        entries(&world.root()).is_empty(),
        "a cancelled install throws its work away, the wheel with it: {:?}",
        entries(&world.root())
    );
    if let Ok(text) = std::fs::read_to_string(format!("{}.sleeper", world.log.display())) {
        if let Ok(pid) = text.trim().parse() {
            crate::testkit::procs::kill_quietly(pid);
        }
    }
}

// ---- the real thing -------------------------------------------------------------------------------------------------

#[test]
#[ignore = "manual: needs the internet (cargo test -p quickai p3_t91 -- --ignored)"]
fn p3_t91_the_real_index_still_has_the_wheel_the_document_pins() {
    use quickai_core::acquire;
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry
        .engine("openwebui")
        .expect("the Open WebUI document");
    let tag = acquire::tested_tag(engine).expect("a tested release");
    let one =
        crate::testkit::http::get(&format!("https://pypi.org/pypi/open-webui/{tag}/json")).json();
    let wheel = one["urls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|f| f["filename"] == format!("open_webui-{tag}-py3-none-any.whl"))
        .expect("the tested release has its wheel on the index");
    assert_eq!(wheel["yanked"], false, "the tested wheel was withdrawn");
    assert_eq!(
        wheel["digests"]["sha256"].as_str().unwrap(),
        real_pin(),
        "the index lists another file than the one that was pinned"
    );
    assert_eq!(
        one["info"]["requires_python"], "<3.13.0a1,>=3.11",
        "the Pythons the document looks for are the ones the package says it runs on"
    );
    // and the file itself is what the index says it is
    let url = wheel["url"].as_str().unwrap();
    let bytes = ureq::get(url)
        .header("User-Agent", "quickai-test")
        .call()
        .unwrap_or_else(|e| panic!("{url}: {e}"))
        .into_body()
        .with_config()
        .limit(300 * 1024 * 1024)
        .read_to_vec()
        .unwrap_or_else(|e| panic!("{url}: {e}"));
    assert_eq!(sha256_hex(&bytes), real_pin());
}

#[test]
#[ignore = "manual: needs the internet, a Python 3.11 or 3.12 with the venv module, about 6 GB on a disk and about 5 minutes; QUICKAI_T90_DIR (a folder on a disk: /tmp may be memory) says where; cargo test -p quickai p3_t90 -- --ignored --nocapture"]
fn p3_t90_the_real_open_webui_is_installed_started_and_answers_through_the_applications_own_code() {
    use crate::events::ServerState;
    use crate::launch::{self, MachineFacts};
    use crate::prims::feed::{spawn_lookup, BuildLookupEvent, LookupSpec};
    use crate::runtime::session::Session;
    use crate::testkit::fake_ollama::FakeOllama;
    use crate::testkit::http;
    use quickai_core::acquire;
    use quickai_core::plan::{Parent, Selection};
    use serde_json::json;

    let work = PathBuf::from(
        std::env::var("QUICKAI_T90_DIR").expect("QUICKAI_T90_DIR: a folder on a disk"),
    );
    std::fs::create_dir_all(&work).unwrap();
    // what the real thing is told not to touch, as it was before: the folder the weights of its default
    // embedding model would have gone in
    let hf = directories::BaseDirs::new()
        .map(|d| d.home_dir().join(".cache/huggingface"))
        .unwrap();
    let hf_before = std::fs::read_dir(&hf).map(|d| d.count()).ok();

    // this machine, as the application sees it, with a data directory of this test's own
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry
        .engine("openwebui")
        .expect("the Open WebUI document");
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    let mut facts = launch::host_facts(Path::new(""), machine, None);
    facts.data_dir = work.join("data").display().to_string();
    println!(
        "{}",
        acquire::install_summary(engine, &facts).unwrap_or_default()
    );

    // 1. the list, from the real index, and the tested release in it
    let rx = spawn_lookup(
        LookupSpec::from_engine(engine, &facts).expect("a feed"),
        || {},
    );
    let mut builds = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(BuildLookupEvent::Found(b)) => builds.push(b),
            Ok(BuildLookupEvent::Status(s)) => println!("[look-up] {s}"),
            Ok(BuildLookupEvent::Done) => break,
            Ok(BuildLookupEvent::Failed) => panic!("the look-up failed"),
            Err(e) => panic!("the look-up did not finish: {e}"),
        }
    }
    println!(
        "builds: {:?}",
        builds
            .iter()
            .map(|b| (&b.tag, b.tested))
            .collect::<Vec<_>>()
    );
    let build = builds
        .iter()
        .find(|b| b.tested)
        .expect("the tested release is offered");
    assert!(build.pin.is_some() && !build.conflict);

    // 2. the install, as the manager's button asks for it
    let spec = InstallSpec::from_engine(engine, &facts).expect("an install spec");
    let started = Instant::now();
    let launcher = install_blocking(&spec, &build.install_request(&[]), &|event| match event {
        crate::jobs::JobEvent::Status(line) => {
            println!("[install {:>4}s] {line}", started.elapsed().as_secs())
        }
        crate::jobs::JobEvent::Detail(line) => {
            println!("[install {:>4}s]   {line}", started.elapsed().as_secs())
        }
        _ => {}
    })
    .unwrap_or_else(|e| panic!("the install failed: {e}"));
    println!(
        "installed {} in {} s",
        launcher.display(),
        started.elapsed().as_secs()
    );
    let build_dir = launcher.parent().unwrap().to_path_buf();
    assert_eq!(
        entries(&build_dir),
        [
            launcher.file_name().unwrap().to_string_lossy().to_string(),
            "venv".to_string()
        ]
    );

    // 3. started by the session, from the plan the document makes, beside an Ollama
    let ollama = FakeOllama::start();
    ollama.add_model("qwen3:4b", 2_500_000_000);
    let port = crate::testkit::free_port();
    let selection = Selection::new("openwebui").set("openwebui.port", port);
    facts.binary = Some(launcher.display().to_string());
    facts.parent = Some(Parent {
        id: "ollama".into(),
        port: ollama.port(),
    });
    let plan = quickai_core::plan::plan(&registry, &selection, &facts).expect("a plan");
    let mut session = Session::new();
    session.start(&plan, "", || {});
    let up = Instant::now();
    loop {
        session.poll(&mut |line| println!("[open-webui] {line}"), &mut |_| {});
        match session.state() {
            ServerState::Running => break,
            ServerState::Crashed | ServerState::Idle => panic!("Open WebUI did not start"),
            _ => {}
        }
        assert!(
            up.elapsed() < Duration::from_secs(300),
            "not up in 5 minutes"
        );
        std::thread::sleep(Duration::from_millis(250));
    }
    println!(
        "Open WebUI is up on port {port} after {} s",
        up.elapsed().as_secs()
    );
    let base = format!("http://127.0.0.1:{port}");
    assert_eq!(http::get(&format!("{base}/health")).json()["status"], true);

    // 4. a person: the first account is the administrator's; it sees the models of the Ollama beside it,
    // and a chat goes through to it
    let account = http::post_json(
        &format!("{base}/api/v1/auths/signup"),
        &json!({"name": "T", "email": "t@example.com", "password": "correct horse battery staple"}),
    );
    assert_eq!(account.status, 200, "{}", account.body);
    let token = account.json()["token"].as_str().unwrap().to_string();
    assert_eq!(account.json()["role"], "admin");
    let models = http::get_with_token(&format!("{base}/api/models"), &token).json();
    let ids: Vec<&str> = models["data"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|m| m["id"].as_str())
        .collect();
    assert!(ids.contains(&"qwen3:4b"), "{ids:?}");
    let chat = http::post_json_with_token(
        &format!("{base}/api/chat/completions"),
        &json!({"model": "qwen3:4b", "messages": [{"role": "user", "content": "hi"}], "stream": false}),
        &token,
    );
    assert_eq!(chat.status, 200, "{}", chat.body);
    assert!(
        !ollama.requests_to("/api/chat").is_empty(),
        "the chat reached the Ollama beside it"
    );

    // 5. stopped with nothing left: the port is closed, and everything it made is in its own root
    session.stop_blocking();
    assert!(
        http::try_get(&format!("{base}/health")).is_err(),
        "it is still answering"
    );
    let root = work.join("data/openwebui");
    for kept in ["data/webui.db", "data/.webui_secret_key", "home", "cache"] {
        assert!(root.join(kept).exists(), "{kept} is not in its root");
    }
    assert!(
        !std::path::Path::new(".webui_secret_key").exists(),
        "its secret key was written where QuickAI was started from"
    );
    assert_eq!(
        std::fs::read_dir(&hf).map(|d| d.count()).ok(),
        hf_before,
        "something was added to the person's own Hugging Face folder"
    );
}

#[test]
#[ignore = "manual: needs the internet, a Python 3.11 or 3.12 with the venv module, about 6 GB on a disk and about 8 minutes; QUICKAI_T90_DIR (a folder on a disk: /tmp may be memory) says where; cargo test -p quickai p3_a67 -- --ignored --nocapture"]
fn p3_a67_the_real_open_webui_started_twice_keeps_the_address_of_ollama_it_was_first_given() {
    use crate::events::ServerState;
    use crate::launch::{self, MachineFacts};
    use crate::prims::feed::{spawn_lookup, BuildLookupEvent, LookupSpec};
    use crate::runtime::session::Session;
    use crate::testkit::fake_ollama::FakeOllama;
    use crate::testkit::http;
    use quickai_core::acquire;
    use quickai_core::plan::{Parent, Selection};
    use serde_json::json;

    // W16 of docs/engine-facts.md: Open WebUI keeps the address of Ollama it is given at its first start in a
    // database of its own (`models/config.py` of 0.11.4: "Existing DB values take precedence over defaults").
    // This starts the real one twice, beside an Ollama that is somewhere else the second time, and says which it calls.
    let work = PathBuf::from(
        std::env::var("QUICKAI_T90_DIR").expect("QUICKAI_T90_DIR: a folder on a disk"),
    );
    std::fs::create_dir_all(&work).unwrap();
    let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
    let engine = registry
        .engine("openwebui")
        .expect("the Open WebUI document");
    let machine = MachineFacts::detect(crate::hardware::total_ram_bytes(), &crate::gpu::detect());
    let mut facts = launch::host_facts(Path::new(""), machine, None);
    facts.data_dir = work.join("data").display().to_string();

    // the tested release, installed as the manager's button asks for it
    let rx = spawn_lookup(
        LookupSpec::from_engine(engine, &facts).expect("a feed"),
        || {},
    );
    let mut builds = Vec::new();
    loop {
        match rx.recv_timeout(Duration::from_secs(60)) {
            Ok(BuildLookupEvent::Found(b)) => builds.push(b),
            Ok(BuildLookupEvent::Status(s)) => println!("[look-up] {s}"),
            Ok(BuildLookupEvent::Done) => break,
            Ok(BuildLookupEvent::Failed) => panic!("the look-up failed"),
            Err(e) => panic!("the look-up did not finish: {e}"),
        }
    }
    let build = builds
        .iter()
        .find(|b| b.tested)
        .expect("the tested release is offered");
    let spec = InstallSpec::from_engine(engine, &facts).expect("an install spec");
    let started = Instant::now();
    let launcher = install_blocking(&spec, &build.install_request(&[]), &|event| {
        if let crate::jobs::JobEvent::Status(line) = event {
            println!("[install {:>4}s] {line}", started.elapsed().as_secs());
        }
    })
    .unwrap_or_else(|e| panic!("the install failed: {e}"));
    facts.binary = Some(launcher.display().to_string());

    // one start beside the Ollama that is at `address`, until it answers; returns the session and its port
    let start = |ollama: &FakeOllama, label: &str| -> (Session, u16) {
        let port = crate::testkit::free_port();
        let selection = Selection::new("openwebui").set("openwebui.port", port);
        let mut facts = facts.clone();
        facts.parent = Some(Parent {
            id: "ollama".into(),
            port: ollama.port(),
        });
        let plan = quickai_core::plan::plan(&registry, &selection, &facts).expect("a plan");
        let mut session = Session::new();
        session.start(&plan, "", || {});
        let up = Instant::now();
        loop {
            session.poll(&mut |line| println!("[{label}] {line}"), &mut |_| {});
            match session.state() {
                ServerState::Running => break,
                ServerState::Crashed | ServerState::Idle => {
                    panic!("Open WebUI did not start ({label})")
                }
                _ => {}
            }
            assert!(
                up.elapsed() < Duration::from_secs(300),
                "not up in 5 minutes ({label})"
            );
            std::thread::sleep(Duration::from_millis(250));
        }
        (session, port)
    };
    let model_ids = |base: &str, token: &str| -> Vec<String> {
        http::get_with_token(&format!("{base}/api/models"), token).json()["data"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|m| m["id"].as_str().map(str::to_string))
                    .collect()
            })
            .unwrap_or_default()
    };

    // the first start: Ollama A; the first account; A's models
    let a = FakeOllama::start();
    a.add_model("model-of-a:1b", 1_000_000_000);
    let (mut first, port) = start(&a, "first");
    let base = format!("http://127.0.0.1:{port}");
    let account = http::post_json(
        &format!("{base}/api/v1/auths/signup"),
        &json!({"name": "T", "email": "t@example.com", "password": "correct horse battery staple"}),
    );
    assert_eq!(account.status, 200, "{}", account.body);
    let token = account.json()["token"].as_str().unwrap().to_string();
    let ids = model_ids(&base, &token);
    println!("first start, models: {ids:?}");
    assert!(ids.contains(&"model-of-a:1b".to_string()), "{ids:?}");
    first.stop_blocking();

    // the second start: the Ollama is somewhere else (B), and A is gone
    a.go_down();
    let b = FakeOllama::start();
    b.add_model("model-of-b:1b", 1_000_000_000);
    assert_ne!(a.port(), b.port());
    let (mut second, port) = start(&b, "second");
    let base = format!("http://127.0.0.1:{port}");
    let login = http::post_json(
        &format!("{base}/api/v1/auths/signin"),
        &json!({"email": "t@example.com", "password": "correct horse battery staple"}),
    );
    assert_eq!(login.status, 200, "{}", login.body);
    let token = login.json()["token"].as_str().unwrap().to_string();
    let ids = model_ids(&base, &token);
    let asked_b = !b.requests_to("/api/tags").is_empty();
    println!("second start, models: {ids:?}; the new Ollama (B) was asked: {asked_b}");
    second.stop_blocking();
    // what is settled: it goes on calling the address of its first start, so the models of B are not there and B is not asked
    assert!(
        !ids.contains(&"model-of-b:1b".to_string()) && !asked_b,
        "Open WebUI used the address it was given the second time: W16 is not what the source says. models {ids:?}, B asked {asked_b}"
    );
    let _ = acquire::install_summary(engine, &facts);
}
