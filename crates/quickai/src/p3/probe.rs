//! Group L of the test plan: asking an installed engine what version it is (`exec.probe@1`, work
//! package 3.11), against what the real `ollama --version` prints in the two situations it can be
//! run in (docs/engine-facts.md, A17).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
use quickai_core::plan::HostFacts;
use quickai_core::registry::Registry;

use crate::prims::exec_probe::{probe_version, VersionCache};
use crate::prims::locate::{locate, LocateEnv, Located};
use crate::testkit::procs::write_script;
use crate::testkit::{scratch, Scratch};

/// `version is (\S+)`: the pattern of A17, which reads both forms.
const PATTERN: &str = r"version is (\S+)";

fn prints(
    dir: &std::path::Path,
    name: &str,
    stdout: &str,
    stderr: &str,
    exit: i32,
) -> std::path::PathBuf {
    let out = dir.join(format!("{name}.out"));
    let err = dir.join(format!("{name}.err"));
    std::fs::write(&out, stdout).unwrap();
    std::fs::write(&err, stderr).unwrap();
    write_script(
        dir,
        name,
        &format!(
            "cat \"{}\"\ncat \"{}\" >&2\nexit {exit}",
            out.display(),
            err.display()
        ),
    )
}

/// What the real binary printed with no server: a warning, then the version, both on stdout.
const NO_SERVER: &str =
    "Warning: could not connect to a running Ollama instance\nWarning: client version is 0.35.1\n";
const WITH_SERVER: &str = "ollama version is 0.35.1\n";

#[cfg(unix)]
#[test]
fn p3_l05_the_version_is_read_from_both_forms_of_what_the_real_ollama_prints() {
    let dir = scratch("probe");
    for (name, text) in [("with-server", WITH_SERVER), ("no-server", NO_SERVER)] {
        let bin = prints(&dir, name, text, "", 0);
        assert_eq!(
            probe_version(&bin, &["--version"], PATTERN, Duration::from_secs(5)).as_deref(),
            Some("0.35.1"),
            "{name}"
        );
    }
}

#[cfg(unix)]
#[test]
fn p3_l05b_stderr_counts_a_failing_program_that_still_says_it_counts_and_one_that_says_nothing_does_not(
) {
    let dir = scratch("probe-b");
    let on_stderr = prints(&dir, "stderr", "", WITH_SERVER, 0);
    assert_eq!(
        probe_version(&on_stderr, &["--version"], PATTERN, Duration::from_secs(5)).as_deref(),
        Some("0.35.1")
    );
    let exits_1 = prints(&dir, "exit1", WITH_SERVER, "", 1);
    assert_eq!(
        probe_version(&exits_1, &["--version"], PATTERN, Duration::from_secs(5)).as_deref(),
        Some("0.35.1"),
        "the exit status does not matter: the program has the fact or it does not"
    );
    let silent = prints(&dir, "silent", "", "", 0);
    assert_eq!(
        probe_version(&silent, &["--version"], PATTERN, Duration::from_secs(5)),
        None
    );
    let other = prints(&dir, "other", "something else entirely\n", "", 0);
    assert_eq!(
        probe_version(&other, &["--version"], PATTERN, Duration::from_secs(5)),
        None
    );
    assert_eq!(
        probe_version(
            &dir.join("not-there"),
            &["--version"],
            PATTERN,
            Duration::from_secs(5)
        ),
        None
    );
}

#[cfg(unix)]
#[test]
fn p3_l05c_a_program_that_hangs_is_killed_at_the_timeout_and_gives_no_version() {
    let dir = scratch("probe-c");
    let hang = write_script(
        &dir,
        "hang",
        "echo \"ollama version is 9.9.9\"\nexec sleep 60",
    );
    let started = Instant::now();
    let got = probe_version(&hang, &["--version"], PATTERN, Duration::from_millis(400));
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert_eq!(got, None, "a program that does not finish has not answered");
    // and it was killed, not left running
    std::thread::sleep(Duration::from_millis(200));
    let alive = std::process::Command::new("pgrep")
        .args(["-f", &hang.display().to_string()])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&alive.stdout).trim().is_empty(),
        "the probe left the program running"
    );
}

// ---- finding the binary (L01 to L04) -----------------------------------------------------------------
//
// The strategies are read from a document, as the application reads them, and given a machine to
// look at: the system, the data and home directories and the environment are all passed in, so a test
// plays Windows or a Mac without leaving Linux and without touching the machine it runs on.

fn locate_doc(extra: &str) -> String {
    format!(
        r#"schema = 1
kind = "engine"
id = "found"
requires = {{ primitives = ["spawn@1"] }}
[engine]
name = "Found"
[launch]
strategies = ["spawn"]
[launch.spawn]
program = "x"
[locate]
strategies = [
{extra}
]
"#
    )
}

fn strategies_of(doc: &str) -> Vec<quickai_core::doc::engine::LocateStrategy> {
    let source = MemorySource::new(&[("embedded:engines/found.toml", doc)]);
    let registry = Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &crate::prims::known(),
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    registry.engine("found").unwrap().locate.clone()
}

/// A machine to look at: a directory standing for its disk, a system, and an environment.
struct Machine {
    disk: Scratch,
    os: &'static str,
    vars: std::collections::BTreeMap<String, String>,
}

impl Machine {
    fn new(os: &'static str) -> Machine {
        Machine {
            disk: scratch("locate"),
            os,
            vars: Default::default(),
        }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.disk.join(relative)
    }

    fn file(&self, relative: &str, executable: bool) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"#!/bin/sh\n").unwrap();
        let mode = if executable { 0o755 } else { 0o644 };
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(mode)).unwrap();
        path
    }

    fn var(&mut self, name: &str, value: impl Into<String>) {
        self.vars.insert(name.to_string(), value.into());
    }

    fn look(&self, strategies: &[quickai_core::doc::engine::LocateStrategy]) -> Option<Located> {
        let facts = HostFacts {
            os: self.os.into(),
            data_dir: self.path("data").display().to_string(),
            home: self.path("home").display().to_string(),
            ..Default::default()
        };
        let vars = self.vars.clone();
        locate(
            strategies,
            &LocateEnv {
                exe_dir: None,
                dev_root: &self.path("dev"),
                setting: &|_| None,
                facts: &facts,
                var: &|name| vars.get(name).cloned(),
            },
        )
    }
}

/// Sets the modification time of a directory, which is how the newest build is told.
fn set_modified(path: &Path, seconds_ago: u64) {
    let when = std::time::SystemTime::now() - Duration::from_secs(seconds_ago);
    std::fs::File::open(path)
        .unwrap()
        .set_modified(when)
        .unwrap();
}

const INSTALLED: &str = r#"  { use = "installed", root = "${data}/ollama", pick = "newest", binary = { linux = "bin/ollama", macos = "bin/ollama", windows = "ollama.exe" } },"#;

#[test]
fn p3_l01_installed_takes_the_newest_build_and_skips_what_is_not_one() {
    let strategies = strategies_of(&locate_doc(INSTALLED));
    let m = Machine::new("linux");
    assert_eq!(
        m.look(&strategies),
        None,
        "a root that is not there has no build"
    );

    let old = m.file("data/ollama/v0.34.4/bin/ollama", true);
    let new = m.file("data/ollama/v0.35.1/bin/ollama", true);
    set_modified(&m.path("data/ollama/v0.34.4"), 3000);
    set_modified(&m.path("data/ollama/v0.35.1"), 100);
    let got = m.look(&strategies).expect("a build");
    assert_eq!(
        (got.path.clone(), got.exists, got.strategy),
        (new.clone(), true, "installed")
    );
    assert_ne!(got.path, old);

    // what is newer but not a build: an install in progress, a directory with no executable, a file
    m.file("data/ollama/v0.40.0.partial/bin/ollama", true);
    std::fs::create_dir_all(m.path("data/ollama/v0.39.0/lib")).unwrap();
    m.file("data/ollama/stray-file", false);
    for dir in ["v0.40.0.partial", "v0.39.0"] {
        set_modified(&m.path(&format!("data/ollama/{dir}")), 1);
    }
    assert_eq!(
        m.look(&strategies).unwrap().path,
        new,
        "something that is not a build was taken"
    );

    // the path of the executable is the document's for this system
    let windows = Machine::new("windows");
    let exe = windows.file("data/ollama/v0.35.1/ollama.exe", false);
    assert_eq!(windows.look(&strategies).unwrap().path, exe);
    let mac = Machine::new("macos");
    let bin = mac.file("data/ollama/v0.35.1/bin/ollama", true);
    assert_eq!(mac.look(&strategies).unwrap().path, bin);
    // a system the document gives no path for finds nothing
    assert_eq!(Machine::new("freebsd").look(&strategies), None);
}

#[test]
fn p3_l02_path_search_finds_a_program_a_person_could_start_in_the_directories_of_the_path() {
    let strategies = strategies_of(&locate_doc(
        r#"  { use = "path_search", name = "ollama" },"#,
    ));
    let mut m = Machine::new("linux");
    // a directory called `ollama` is no match; a file that cannot be executed is none either
    std::fs::create_dir_all(m.path("a/ollama")).unwrap();
    m.file("b/ollama", false);
    let first = m.file("c/ollama", true);
    m.file("d/ollama", true);
    // empty and relative entries are ignored, whatever they would find
    let relative = {
        let there = m.file("rel/ollama", true);
        let depth = std::env::current_dir().unwrap().components().count();
        PathBuf::from("../".repeat(depth)).join(there.strip_prefix("/").unwrap())
    };
    assert!(relative.is_relative() && std::fs::metadata(&relative).is_ok());
    let join = |dirs: &[&str]| {
        dirs.iter()
            .map(|d| d.to_string())
            .collect::<Vec<_>>()
            .join(":")
    };
    let path = join(&[
        "",
        relative.parent().unwrap().to_str().unwrap(),
        m.path("a").to_str().unwrap(),
        m.path("b").to_str().unwrap(),
        m.path("c").to_str().unwrap(),
        m.path("d").to_str().unwrap(),
    ]);
    m.var("PATH", path);
    let got = m
        .look(&strategies)
        .expect("the first executable on the path");
    assert_eq!((got.path, got.strategy), (first, "path_search"));
    // no PATH, and a PATH with nothing, find nothing
    m.vars.remove("PATH");
    assert_eq!(m.look(&strategies), None);
    m.var("PATH", "");
    assert_eq!(m.look(&strategies), None);

    // Windows: the extensions of PATHEXT after the name, executable bits mean nothing
    let mut w = Machine::new("windows");
    let exe = w.file("bin/ollama.exe", false);
    w.var("PATH", w.path("bin").display().to_string());
    assert_eq!(
        w.look(&strategies).map(|l| l.path),
        Some(exe.clone()),
        "with no PATHEXT the usual default (.COM, .EXE, .BAT, .CMD) applies"
    );
    w.var("PATHEXT", ".COM;.EXE;.BAT");
    assert_eq!(w.look(&strategies).unwrap().path, exe);
    w.var("PATHEXT", ".BAT;.CMD");
    assert_eq!(w.look(&strategies), None, ".exe is not in this PATHEXT");
    // a name that already ends in an extension is taken as written
    let named = strategies_of(&locate_doc(
        r#"  { use = "path_search", name = "ollama.exe" },"#,
    ));
    w.var("PATHEXT", ".BAT;.CMD");
    assert_eq!(w.look(&named).unwrap().path, exe);
}

#[test]
fn p3_l03_known_paths_are_the_systems_own_with_tilde_and_variables_filled_in() {
    let m0 = Machine::new("linux");
    let places = |m: &Machine| {
        strategies_of(&locate_doc(&format!(
            r#"  {{ use = "known_paths", linux = ["{missing}", "{usr}", "~/bin/ollama"], macos = ["{mac}"], windows = ['%LOCALAPPDATA%\Programs\Ollama\ollama.exe', '%UNSET%\x.exe'] }},"#,
            missing = m.path("nowhere/ollama").display(),
            usr = m.path("usr/local/bin/ollama").display(),
            mac = m.path("Applications/ollama").display(),
        )))
    };
    let strategies = places(&m0);
    assert_eq!(
        m0.look(&strategies),
        None,
        "a place that does not exist is no match"
    );

    let usr = m0.file("usr/local/bin/ollama", true);
    let got = m0.look(&strategies).unwrap();
    assert_eq!((got.path, got.strategy), (usr, "known_paths"));
    // the first place that has a file wins; `~` is the home directory
    let home = m0.file("home/bin/ollama", true);
    std::fs::remove_file(m0.path("usr/local/bin/ollama")).unwrap();
    assert_eq!(m0.look(&strategies).unwrap().path, home);

    // another system's list is not this system's
    let mac = Machine::new("macos");
    let app = mac.file("Applications/ollama", true);
    assert_eq!(mac.look(&places(&mac)).unwrap().path, app);

    // %NAME% is the environment's; a place that needs a variable that is not set is skipped
    let mut w = Machine::new("windows");
    let strategies = places(&w);
    assert_eq!(w.look(&strategies), None);
    w.var("LOCALAPPDATA", w.path("AppData").display().to_string());
    // (a backslash is a plain character on this machine, which is enough to see the expansion)
    let exe = w.file(
        &format!("AppData{}Programs{}Ollama{}ollama.exe", '\\', '\\', '\\'),
        false,
    );
    assert_eq!(w.look(&strategies).unwrap().path, exe);
}

#[test]
fn p3_l04_the_first_strategy_with_a_finding_wins_in_the_documents_order_and_says_which() {
    let doc = locate_doc(&format!(
        "{INSTALLED}\n  {{ use = \"path_search\", name = \"ollama\" }},\n  {{ use = \"known_paths\", linux = [\"{{KNOWN}}\"] }},"
    ));
    let mut m = Machine::new("linux");
    let known = m.path("known/ollama");
    let doc = doc.replace("{KNOWN}", &known.display().to_string());
    let strategies = strategies_of(&doc);
    m.file("known/ollama", true);
    let on_path = m.file("onpath/ollama", true);
    m.var("PATH", m.path("onpath").display().to_string());
    let installed = m.file("data/ollama/v1/bin/ollama", true);

    let got = m.look(&strategies).unwrap();
    assert_eq!((got.path, got.strategy), (installed, "installed"));
    std::fs::remove_dir_all(m.path("data")).unwrap();
    let got = m.look(&strategies).unwrap();
    assert_eq!((got.path, got.strategy), (on_path, "path_search"));
    std::fs::remove_dir_all(m.path("onpath")).unwrap();
    let got = m.look(&strategies).unwrap();
    assert_eq!((got.path, got.strategy), (known, "known_paths"));

    // a strategy that finds nothing does not end the search, whatever it is
    let setting_first = strategies_of(&locate_doc(&format!(
        "  {{ use = \"known_paths\", windows = [\"C:\\\\x\"] }},\n{INSTALLED}"
    )));
    let m = Machine::new("linux");
    let installed = m.file("data/ollama/v1/bin/ollama", true);
    assert_eq!(m.look(&setting_first).unwrap().path, installed);
}

// ---- remembering the version (L06) -------------------------------------------------------------------

#[test]
fn p3_l06_the_version_is_cached_by_path_and_modification_time_so_asking_again_starts_nothing() {
    let dir = scratch("probe-cache");
    let counter = dir.join("starts.txt");
    let script = |version: &str| {
        write_script(
            &dir,
            "ollama",
            &format!(
                "echo started >> \"{}\"\necho \"ollama version is {version}\"",
                counter.display()
            ),
        )
    };
    let starts = || {
        std::fs::read_to_string(&counter)
            .map(|t| t.lines().count())
            .unwrap_or(0)
    };
    let cache = VersionCache::default();
    let ask =
        |binary: &Path| cache.version(binary, &["--version"], PATTERN, Duration::from_secs(5));

    let binary = script("1.2.3");
    assert_eq!(ask(&binary).as_deref(), Some("1.2.3"));
    assert_eq!(ask(&binary).as_deref(), Some("1.2.3"));
    assert_eq!(ask(&binary).as_deref(), Some("1.2.3"));
    assert_eq!(
        starts(),
        1,
        "the program was started again for a question it had answered"
    );

    // the same place, another build: asked again
    script("1.2.40");
    assert_eq!(ask(&binary).as_deref(), Some("1.2.40"));
    assert_eq!(starts(), 2);
    assert_eq!(ask(&binary).as_deref(), Some("1.2.40"));
    assert_eq!(starts(), 2);

    // the same bytes with a newer modification time are asked again too
    let file = std::fs::File::options().write(true).open(&binary).unwrap();
    file.set_modified(std::time::SystemTime::now() + Duration::from_secs(120))
        .unwrap();
    drop(file);
    assert_eq!(ask(&binary).as_deref(), Some("1.2.40"));
    assert_eq!(
        starts(),
        3,
        "a changed modification time did not make it ask again"
    );

    // another way of asking is another question, not the answer to the first
    assert_eq!(
        cache
            .version(
                &binary,
                &["--version"],
                r"(\S+) version",
                Duration::from_secs(5)
            )
            .as_deref(),
        Some("ollama")
    );
    assert_eq!(
        starts(),
        4,
        "a different pattern was answered from the remembered one"
    );

    // a file that is not there has no version, and no answer is not remembered
    assert_eq!(ask(&dir.join("missing")), None);
    let quiet = write_script(&dir, "quiet", "exit 0");
    assert_eq!(ask(&quiet), None);
    std::fs::write(&quiet, "#!/bin/sh\necho \"ollama version is 9.0.0\"\n").unwrap();
    assert_eq!(
        ask(&quiet).as_deref(),
        Some("9.0.0"),
        "a failed probe was remembered"
    );
}
