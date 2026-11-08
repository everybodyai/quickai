//! Crashes: the panic hook that writes a problem report to this computer's disk when QuickAI panics, with no network
//! and nothing slow. The next start shows it (`super::pending_crashes`); it never leaves the computer by itself.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use quickai_core::report::clean::Cleaner;
use quickai_core::report::{measure, Environment, Kind, Report};

/// What the window was doing, kept up to date by it so that a crash report can say: the engine, its state and the
/// windows that are open.
#[derive(Clone, Debug, Default)]
pub struct CrashContext {
    pub engine: Option<String>,
    /// One of `idle`, `starting`, `running`, `stopping`, `crashed`.
    pub engine_state: Option<&'static str>,
    /// The windows that are open (`add_model`, `settings`...).
    pub open_windows: Vec<&'static str>,
    /// What the window was showing that runs on without it, of `measure::RUNNING` (`none` is left out).
    pub running: Option<&'static str>,
    /// What the machine said of its memory and its waits at the crash: the hook reads it, as late as it can.
    pub machine: Option<super::machine::MachineNow>,
}

/// Functions of the panic machinery itself, which say nothing about where QuickAI failed.
const NOISE: &[&str] = &[
    "std::backtrace",
    "std::panicking",
    "core::panicking",
    "rust_begin_unwind",
    "std::sys::backtrace",
    "std::rt::",
    "__rust",
    "quickai_runtime::report::crash",
    "<alloc::boxed::Box<F,A> as core::ops::function::Fn",
];

/// The function names of a backtrace as `std::backtrace::Backtrace` writes it (`  3: quickai::app::App::ui`), with
/// their hashes taken off and the panic machinery left out, at most 64.
pub fn frames(backtrace: &str) -> Vec<String> {
    backtrace
        .lines()
        .filter_map(|line| {
            let line = line.trim_start();
            let (number, name) = line.split_once(": ")?;
            number
                .chars()
                .all(|c| c.is_ascii_digit())
                .then_some(name.trim())
        })
        .map(strip_hash)
        .filter(|name| !NOISE.iter().any(|noise| name.starts_with(noise)))
        .map(|name| name.chars().take(300).collect())
        .take(64)
        .collect()
}

fn strip_hash(name: &str) -> &str {
    match name.rfind("::h") {
        Some(at)
            if name.len() - at == 19 && name[at + 3..].chars().all(|c| c.is_ascii_hexdigit()) =>
        {
            &name[..at]
        }
        _ => name,
    }
}

/// What a panic says, made into a report: the message and the location cleaned, the thread, the frames, how long
/// QuickAI had been open, and what the window was doing.
#[allow(clippy::too_many_arguments)]
pub fn crash_report(
    environment: &Environment,
    message: &str,
    location: Option<(&str, u32, u32)>,
    thread: &str,
    backtrace: &str,
    context: &CrashContext,
    minutes_open: u64,
    cleaner: &Cleaner,
) -> Report {
    let mut report =
        Report::new(Kind::Crash, environment.clone()).detail("Message", cleaner.clean(message));
    if let Some((file, line, column)) = location {
        let file: String = cleaner
            .clean(file)
            .chars()
            .map(|c| match c {
                // the same place reads the same on Windows, whose builds write `crates\\quickai\\src\\...`
                '\\' => '/',
                c if c.is_whitespace() => '_',
                c => c,
            })
            .collect();
        report = report.detail("Where", format!("{file}:{line}:{column}"));
    }
    report = report
        .detail(
            "Thread",
            cleaner.clean(thread).chars().take(100).collect::<String>(),
        )
        .detail("Open for (minutes)", measure::minutes_bucket(minutes_open));
    if let Some(engine) = context
        .engine
        .as_deref()
        .filter(|e| measure::is_plain_name(e))
    {
        report = report.detail("Engine", engine);
    }
    if let Some(state) = context.engine_state {
        report = report.detail("Engine state", state);
    }
    if let Some(running) = context.running {
        report = report.detail("Going on", running);
    }
    if !context.open_windows.is_empty() {
        report = report.detail("Windows open", context.open_windows.join(", "));
    }
    if let Some(machine) = &context.machine {
        report = super::machine::with_machine(report, machine, false);
    }
    report.backtrace = frames(backtrace);
    report
}

/// Writes a report to `dir/crash-<seconds>-<process>.txt`, whole or not at all.
pub fn write_report(dir: &Path, report: &Report, now: u64) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let name = format!("crash-{now}-{}", std::process::id());
    let path = dir.join(format!("{name}.txt"));
    let part = dir.join(format!("{name}.txt.part"));
    std::fs::write(&part, report.text())?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// What the hook needs, prepared before anything can panic.
pub(super) struct Hook {
    pub dir: PathBuf,
    pub environment: Environment,
    pub cleaner: Cleaner,
    pub context: Arc<Mutex<CrashContext>>,
    pub started: u64,
    pub clock: Arc<dyn Fn() -> u64 + Send + Sync>,
}

impl Hook {
    /// The report of the panic `info`, written to the crash folder. Nothing here may panic: a panic inside a panic
    /// hook aborts the program.
    pub fn record(&self, info: &std::panic::PanicHookInfo<'_>) {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| s.to_string())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic with a value that is not text".to_string());
        let location = info.location().map(|l| (l.file(), l.line(), l.column()));
        let thread = std::thread::current();
        let thread = thread.name().unwrap_or("<unnamed>");
        let backtrace = std::backtrace::Backtrace::force_capture().to_string();
        // a lock held by the thread that panicked is not waited for
        let mut context = self
            .context
            .try_lock()
            .map(|c| c.clone())
            .unwrap_or_default();
        // a few small files of /proc, read now: how the memory was is part of why a program fails
        context.machine = Some(super::machine::now(None));
        let now = (self.clock)();
        let minutes = now.saturating_sub(self.started) / 60;
        let report = crash_report(
            &self.environment,
            &message,
            location,
            thread,
            &backtrace,
            &context,
            minutes,
            &self.cleaner,
        );
        let _ = write_report(&self.dir, &report, now);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKTRACE: &str = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/abc/library/std/src/../../backtrace/src/backtrace/libunwind.rs:117:9
   1: std::backtrace::Backtrace::force_capture
   2: quickai_runtime::report::crash::Hook::record
   3: std::panicking::rust_panic_with_hook
   4: core::panicking::panic_fmt
   5: quickai::app::catalog_view::<impl quickai::app::App>::show_catalog_window::h0123456789abcdef
             at ./crates/quickai/src/app/catalog_view.rs:412:31
   6: quickai::app::shell::<impl quickai::app::App>::show_window
   7: main";

    #[test]
    fn the_frames_are_the_function_names_without_the_panic_machinery() {
        assert_eq!(
            frames(BACKTRACE),
            vec![
                "quickai::app::catalog_view::<impl quickai::app::App>::show_catalog_window",
                "quickai::app::shell::<impl quickai::app::App>::show_window",
                "main",
            ]
        );
    }

    fn line<'a>(report: &'a Report, name: &str) -> Option<&'a str> {
        report
            .details
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    #[test]
    fn a_panic_becomes_a_clean_report() {
        let cleaner = Cleaner::new(Some("/home/grace"), Some("grace"), None, &[]);
        let context = CrashContext {
            engine: Some("ollama".into()),
            engine_state: Some("running"),
            open_windows: vec!["recommended"],
            running: Some("download"),
            machine: Some(super::super::machine::MachineNow {
                memory_available: Some(512 * 1024 * 1024),
                ..Default::default()
            }),
        };
        let report = crash_report(
            &Environment::default(),
            "cannot read /home/grace/.quickai/models/secret-model.gguf",
            Some(("crates/quickai/src/app/catalog_view.rs", 412, 31)),
            "main",
            BACKTRACE,
            &context,
            42,
            &cleaner,
        );
        assert_eq!(report.kind, Kind::Crash);
        assert_eq!(
            line(&report, "Message"),
            Some("cannot read ~/.quickai/models/<model file>")
        );
        assert_eq!(
            line(&report, "Where"),
            Some("crates/quickai/src/app/catalog_view.rs:412:31")
        );
        assert_eq!(line(&report, "Engine"), Some("ollama"));
        assert_eq!(line(&report, "Open for (minutes)"), Some("30-60"));
        assert_eq!(line(&report, "Going on"), Some("download"));
        assert_eq!(line(&report, "Windows open"), Some("recommended"));
        assert_eq!(line(&report, "Memory available (GB)"), Some("<1"));
        assert_eq!(report.backtrace.len(), 3);
        let text = report.text();
        assert!(
            !text.contains("grace") && !text.contains("secret-model"),
            "{text}"
        );
    }

    #[test]
    fn a_location_in_a_home_with_a_space_still_fits() {
        let report = crash_report(
            &Environment::default(),
            "x",
            Some(("/opt/my crate/src/lib.rs", 1, 2)),
            "worker",
            "",
            &CrashContext::default(),
            0,
            &Cleaner::default(),
        );
        assert_eq!(line(&report, "Where"), Some("/opt/my_crate/src/lib.rs:1:2"));
    }

    #[test]
    fn a_location_on_windows_reads_as_it_does_on_the_other_systems() {
        let report = crash_report(
            &Environment::default(),
            "x",
            Some((r"crates\quickai\src\app\mod.rs", 1, 2)),
            "worker",
            "",
            &CrashContext::default(),
            0,
            &Cleaner::default(),
        );
        assert_eq!(
            line(&report, "Where"),
            Some("crates/quickai/src/app/mod.rs:1:2")
        );
    }

    #[test]
    fn a_report_is_written_whole_as_its_text() {
        let dir = std::env::temp_dir().join(format!("quickai-crash-write-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let report = crash_report(
            &Environment::default(),
            "boom",
            None,
            "main",
            "",
            &CrashContext::default(),
            0,
            &Cleaner::default(),
        );
        let path = write_report(&dir, &report, 1_793_351_564).unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), report.text());
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with("crash-1793351564-"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
