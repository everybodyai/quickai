//! Crashes (section 7 of docs/reports-and-community.md): the panic hook that writes a report to disk when QuickAI
//! panics, with no network and nothing slow, and the session markers that tell, at the next start, that the last
//! session ended without closing.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use quickai_core::telemetry::clean::Cleaner;
use quickai_core::telemetry::{measure, schema, Batch, Common, Event};

/// What the window was doing, kept up to date by it so that a crash report can say: the engine, its state and the
/// windows that are open.
#[derive(Clone, Debug, Default)]
pub struct CrashContext {
    pub engine: Option<String>,
    /// One of `idle`, `starting`, `running`, `stopping`, `crashed`.
    pub engine_state: Option<&'static str>,
    /// Of `schema::WINDOWS`.
    pub open_windows: Vec<&'static str>,
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
    "quickai_runtime::telemetry::crash",
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

/// What a panic says, made into the `crash.panic` event: the message and the location cleaned, the thread, the
/// frames, and what the window was doing. `None` if it would not pass the schema (it then is not written at all).
#[allow(clippy::too_many_arguments)]
pub fn crash_event(
    message: &str,
    location: Option<(&str, u32, u32)>,
    thread: &str,
    backtrace: &str,
    context: &CrashContext,
    minutes_open: u64,
    now: u64,
    cleaner: &Cleaner,
) -> Option<Event> {
    let mut event = Event::new("crash.panic", now)
        .with("message", cleaner.clean(message))
        .with(
            "thread",
            cleaner.clean(thread).chars().take(100).collect::<String>(),
        )
        .with("backtrace", frames(backtrace))
        .with("minutes_open", measure::minutes_bucket(minutes_open))
        .with("open_windows", context.open_windows.clone());
    if let Some((file, line, column)) = location {
        let file: String = cleaner
            .clean(file)
            .chars()
            .map(|c| if c.is_whitespace() { '_' } else { c })
            .collect();
        event = event.with("location", format!("{file}:{line}:{column}"));
    }
    if let Some(engine) = context
        .engine
        .as_deref()
        .filter(|e| measure::is_plain_name(e))
    {
        event = event.with("engine", engine);
    }
    if let Some(state) = context.engine_state {
        event = event.with("engine_state", state);
    }
    schema::check(&event).ok().map(|()| event)
}

/// Writes `batch` to `dir/<its id>.json`, whole or not at all.
pub fn write_report(dir: &Path, batch: &Batch) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.json", batch.id));
    let part = dir.join(format!("{}.json.part", batch.id));
    let json = serde_json::to_string_pretty(batch).map_err(std::io::Error::other)?;
    std::fs::write(&part, json)?;
    std::fs::rename(&part, &path)?;
    Ok(path)
}

/// What the hook needs, prepared before anything can panic.
pub(super) struct Hook {
    pub dir: PathBuf,
    pub common: Common,
    pub cleaner: Cleaner,
    pub context: Arc<Mutex<CrashContext>>,
    pub started: u64,
    pub id: Box<dyn Fn() -> String + Send + Sync>,
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
        let context = self
            .context
            .try_lock()
            .map(|c| c.clone())
            .unwrap_or_default();
        let now = (self.clock)();
        let minutes = now.saturating_sub(self.started) / 60;
        if let Some(event) = crash_event(
            &message,
            location,
            thread,
            &backtrace,
            &context,
            minutes,
            now,
            &self.cleaner,
        ) {
            let batch = Batch::new((self.id)(), self.common.clone(), vec![event]);
            let _ = write_report(&self.dir, &batch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BACKTRACE: &str = "   0: std::backtrace_rs::backtrace::libunwind::trace
             at /rustc/abc/library/std/src/../../backtrace/src/backtrace/libunwind.rs:117:9
   1: std::backtrace::Backtrace::force_capture
   2: quickai_runtime::telemetry::crash::Hook::record
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

    #[test]
    fn a_panic_becomes_a_clean_event_of_the_schema() {
        let cleaner = Cleaner::new(Some("/home/grace"), Some("grace"), None, &[]);
        let context = CrashContext {
            engine: Some("ollama".into()),
            engine_state: Some("running"),
            open_windows: vec!["recommended"],
        };
        let event = crash_event(
            "cannot read /home/grace/.quickai/models/secret-model.gguf",
            Some(("crates/quickai/src/app/catalog_view.rs", 412, 31)),
            "main",
            BACKTRACE,
            &context,
            42,
            1_793_351_564,
            &cleaner,
        )
        .expect("an event of the schema");
        assert_eq!(
            event.fields["message"],
            "cannot read ~/.quickai/models/<model file>"
        );
        assert_eq!(
            event.fields["location"],
            "crates/quickai/src/app/catalog_view.rs:412:31"
        );
        assert_eq!(event.fields["engine"], "ollama");
        assert_eq!(event.fields["minutes_open"], "30-60");
        assert_eq!(event.time, "2026-10-30T09:00:00Z");
    }

    #[test]
    fn a_location_in_a_home_with_a_space_still_fits() {
        let cleaner = Cleaner::default();
        let event = crash_event(
            "x",
            Some(("/opt/my crate/src/lib.rs", 1, 2)),
            "worker",
            "",
            &CrashContext::default(),
            0,
            0,
            &cleaner,
        )
        .expect("an event");
        assert_eq!(event.fields["location"], "/opt/my_crate/src/lib.rs:1:2");
    }
}
