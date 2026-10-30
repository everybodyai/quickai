//! Telemetry on this computer (WP4.42, docs/reports-and-community.md): the install id, the queue of events, the
//! batches sent to EverybodyAI and the telemetry log of what was sent, the crash reports, and the level in force.
//!
//! The level is the person's (`Settings::telemetry_level`; not answered is Off), held under the caps of the build,
//! the package and the environment (`caps_from_environment`). Nothing is queued above it and nothing at all is sent at
//! Off. Everything lives in `~/.quickai/telemetry`:
//!
//! ```text
//! install-id        the install id, made the first time anything is sent
//! queue.jsonl       the events waiting, one a line
//! state.json        when the last batch went, how long to wait after a failure, whether the server said stop
//! telemetry.log     what was sent, exactly as it was sent (the last 1 MB)
//! crashes/          crash reports not sent yet; crashes/kept: those the person chose not to send
//! running-<pid>.json  a marker of each session that is open, to tell an unclean exit at the next start
//! ```

mod crash;
pub mod identity;
pub mod send;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};
use std::time::Duration;

use quickai_core::telemetry::clean::Cleaner;
use quickai_core::telemetry::{level, measure, schema, Batch, Event, Level};
use serde::{Deserialize, Serialize};

pub use crash::{crash_event, frames, write_report, CrashContext};
pub use identity::Identity;
pub use send::{Answer, DEFAULT_ENDPOINT};

/// No batch is sent in the first two minutes of a start, except the last one when the app closes.
pub const FIRST_SEND_AFTER: u64 = 120;
/// A batch goes when this many events wait...
pub const BATCH_WHEN: usize = 100;
/// ...or when the last one went this long ago (6 hours).
pub const BATCH_EVERY: u64 = 6 * 3_600;
/// The most events in one batch, and the most bytes.
pub const BATCH_MOST: usize = 500;
pub const BATCH_MOST_BYTES: usize = 64 * 1024;
/// The queue keeps at most this much; the oldest go first.
pub const QUEUE_MOST_BYTES: usize = 1024 * 1024;
/// The telemetry log keeps the last this much.
pub const LOG_MOST_BYTES: usize = 1024 * 1024;
/// After a failure: wait an hour, then six, then a day...
pub const WAITS: [u64; 3] = [3_600, 6 * 3_600, 24 * 3_600];
/// ...and drop what has waited more than three days.
pub const GIVE_UP_AFTER: u64 = 3 * 86_400;
/// A blocked install sends nothing for 30 days.
pub const BLOCKED_FOR: u64 = 30 * 86_400;
/// Crash files are kept 90 days.
pub const CRASHES_KEPT_FOR: u64 = 90 * 86_400;
/// How long a request may take, while the app runs and as it closes.
pub const SEND_TIMEOUT: Duration = Duration::from_secs(15);
pub const CLOSING_TIMEOUT: Duration = Duration::from_secs(2);

/// Seconds since 1970: the clock telemetry reads (a test gives its own).
pub type Clock = Arc<dyn Fn() -> u64 + Send + Sync>;

/// The system's clock.
pub fn system_clock() -> Clock {
    Arc::new(|| {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0)
    })
}

/// Who holds the level down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapBy {
    /// A build without the feature `telemetry`.
    Build,
    /// `telemetry.toml` in the package's configuration folder (an organization's or a packager's).
    Package,
    /// `QUICKAI_TELEMETRY` or `DO_NOT_TRACK`.
    Environment,
}

/// A level that cannot be passed, and who set it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Cap {
    pub level: Level,
    pub by: CapBy,
}

/// The caps of this build, its package and its environment. `package_config` is the package's configuration folder
/// (`ConfigDirs::bundle`): a `telemetry.toml` there with `level = "off"` (or `crash`, `error`) holds the level down.
pub fn caps_from_environment(package_config: Option<&Path>) -> Vec<Cap> {
    let mut caps = Vec::new();
    if !cfg!(feature = "telemetry") {
        caps.push(Cap {
            level: Level::Off,
            by: CapBy::Build,
        });
    }
    if let Some(level) = package_config.and_then(package_cap) {
        caps.push(Cap {
            level,
            by: CapBy::Package,
        });
    }
    let quickai = std::env::var("QUICKAI_TELEMETRY").ok();
    let dnt = std::env::var("DO_NOT_TRACK").ok();
    if let Some(level) = level::env_cap(quickai.as_deref(), dnt.as_deref()) {
        caps.push(Cap {
            level,
            by: CapBy::Environment,
        });
    }
    caps
}

/// The level `<folder>/telemetry.toml` holds the app to: its line `level = "<level>"`.
pub fn package_cap(folder: &Path) -> Option<Level> {
    let text = std::fs::read_to_string(folder.join("telemetry.toml")).ok()?;
    text.lines().find_map(|line| {
        let (key, value) = line.split_once('=')?;
        (key.trim() == "level").then(|| Level::parse(value.trim().trim_matches('"')))?
    })
}

/// Where reports go: `QUICKAI_TELEMETRY_URL`, or `api.everybodyai.dev`.
pub fn endpoint_from_environment() -> String {
    std::env::var("QUICKAI_TELEMETRY_URL")
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| DEFAULT_ENDPOINT.to_string())
}

/// How telemetry starts.
pub struct Setup {
    /// `~/.quickai/telemetry`.
    pub dir: PathBuf,
    /// The level the person chose; `None` while they have not answered.
    pub chosen: Option<Level>,
    pub caps: Vec<Cap>,
    pub endpoint: String,
    pub identity: Identity,
    pub clock: Clock,
}

/// A random id in the shape of a UUID (version 4).
pub fn random_id() -> String {
    let mut bytes = [0u8; 16];
    if getrandom::fill(&mut bytes).is_err() {
        // no system randomness: the hasher's keys are random per process, and the moment and a counter differ
        use std::hash::{BuildHasher, Hasher};
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let state = std::collections::hash_map::RandomState::new();
        for half in 0..2 {
            let mut h = state.build_hasher();
            h.write_u64(COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed));
            h.write_u128(
                std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_nanos())
                    .unwrap_or(0),
            );
            h.write_u32(std::process::id());
            bytes[half * 8..half * 8 + 8].copy_from_slice(&h.finish().to_le_bytes());
        }
    }
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    )
}

/// An event waiting, with the moment it was queued.
#[derive(Clone, Debug, Serialize, Deserialize)]
struct Queued {
    at: u64,
    event: Event,
}

/// What `state.json` keeps between starts.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct SendState {
    #[serde(default)]
    pub last_sent: u64,
    #[serde(default)]
    pub retry_at: u64,
    #[serde(default)]
    pub failures: u32,
    #[serde(default)]
    pub blocked_until: u64,
    /// The version the server said it no longer collects (410).
    #[serde(default)]
    pub retired_version: Option<String>,
}

#[derive(Serialize, Deserialize)]
struct Marker {
    session: String,
    pid: u32,
    started: u64,
}

struct Inner {
    chosen: Option<Level>,
    queue: Vec<Queued>,
    /// What `queue.jsonl` holds, in bytes.
    queue_bytes: usize,
    state: SendState,
    wake: bool,
    closing: bool,
    sender: bool,
}

struct Shared {
    dir: PathBuf,
    endpoint: String,
    identity: Identity,
    session: String,
    started: u64,
    clock: Clock,
    caps: Vec<Cap>,
    unclean: usize,
    inner: Mutex<Inner>,
    wake: Condvar,
    context: Arc<Mutex<CrashContext>>,
}

/// Telemetry for one start of the app. Cloning shares it. `inert` records and sends nothing (the tests of the window).
#[derive(Clone)]
pub struct Telemetry {
    shared: Option<Arc<Shared>>,
}

impl Telemetry {
    /// Telemetry that records nothing, sends nothing and writes nothing.
    pub fn inert() -> Self {
        Self { shared: None }
    }

    /// Telemetry for this start: the queue and the state read back, crash files older than 90 days deleted, the
    /// sessions that ended without closing found (each is one `session.unclean_exit`), and this session's marker
    /// written. Nothing is sent until `spawn_sender` or `send_due`.
    pub fn start(setup: Setup) -> Self {
        let Setup {
            dir,
            chosen,
            caps,
            endpoint,
            identity,
            clock,
        } = setup;
        let now = clock();
        let _ = std::fs::create_dir_all(dir.join("crashes"));
        prune_crashes(&dir, now);
        let unclean = ended_uncleanly(&dir);
        let session = random_id();
        let marker = Marker {
            session: session.clone(),
            pid: std::process::id(),
            started: now,
        };
        if let Ok(json) = serde_json::to_string(&marker) {
            let _ = std::fs::write(
                dir.join(format!("running-{}.json", std::process::id())),
                json,
            );
        }
        let queue = read_queue(&dir);
        let inner = Inner {
            chosen,
            queue_bytes: queue.iter().map(queued_bytes).sum(),
            queue,
            state: std::fs::read_to_string(dir.join("state.json"))
                .ok()
                .and_then(|s| serde_json::from_str(&s).ok())
                .unwrap_or_default(),
            wake: false,
            closing: false,
            sender: false,
        };
        let telemetry = Self {
            shared: Some(Arc::new(Shared {
                dir,
                endpoint,
                identity,
                session,
                started: now,
                clock,
                caps,
                unclean,
                inner: Mutex::new(inner),
                wake: Condvar::new(),
                context: Arc::new(Mutex::new(CrashContext::default())),
            })),
        };
        // what a lower level, or one set while the app was closed, no longer allows goes
        if let Some(shared) = &telemetry.shared {
            let mut inner = shared.lock();
            let level = shared.level_of(&inner);
            inner
                .queue
                .retain(|q| q.event.level().is_some_and(|l| level.allows(l)));
            shared.persist(&mut inner);
        }
        for _ in 0..unclean {
            telemetry.record(telemetry.event("session.unclean_exit"));
        }
        telemetry
    }

    /// Whether this records anything.
    pub fn is_live(&self) -> bool {
        self.shared.is_some()
    }

    /// The level in force.
    pub fn level(&self) -> Level {
        self.shared
            .as_ref()
            .map_or(Level::Off, |s| s.level_of(&s.lock()))
    }

    /// The level the person chose; `None` while they have not answered.
    pub fn chosen(&self) -> Option<Level> {
        self.shared.as_ref().and_then(|s| s.lock().chosen)
    }

    /// The highest level the caps allow (Errors and crashes when there are none), and the caps.
    pub fn cap(&self) -> Level {
        self.caps()
            .iter()
            .map(|c| c.level)
            .min()
            .unwrap_or(Level::Error)
    }

    pub fn caps(&self) -> Vec<Cap> {
        self.shared.as_ref().map_or_else(
            || {
                vec![Cap {
                    level: Level::Off,
                    by: CapBy::Build,
                }]
            },
            |s| s.caps.clone(),
        )
    }

    /// Records what the person chose. What it no longer allows is dropped from the queue; a crash waiting is sent
    /// if it now may be.
    pub fn set_chosen(&self, chosen: Level) {
        let Some(shared) = &self.shared else { return };
        let mut inner = shared.lock();
        inner.chosen = Some(chosen);
        let level = shared.level_of(&inner);
        inner
            .queue
            .retain(|q| q.event.level().is_some_and(|l| level.allows(l)));
        shared.persist(&mut inner);
        inner.wake = true;
        shared.wake.notify_all();
    }

    /// The event `name`, now.
    pub fn event(&self, name: &str) -> Event {
        Event::new(name, self.now())
    }

    /// Seconds since 1970, by telemetry's clock.
    pub fn now(&self) -> u64 {
        self.shared.as_ref().map_or(0, |s| (s.clock)())
    }

    /// How many sessions before this one ended without closing.
    pub fn unclean_sessions(&self) -> usize {
        self.shared.as_ref().map_or(0, |s| s.unclean)
    }

    /// Queues `event` if the level in force allows its level. An event the schema does not have, or of a wrong
    /// shape, is never queued (and in a debug build it stops the program: it is a mistake in QuickAI).
    pub fn record(&self, event: Event) {
        let Some(shared) = &self.shared else { return };
        if let Err(why) = schema::check(&event) {
            debug_assert!(false, "telemetry: {why}");
            return;
        }
        let mut inner = shared.lock();
        let level = shared.level_of(&inner);
        if !event.level().is_some_and(|l| level.allows(l)) {
            return;
        }
        let now = (shared.clock)();
        let queued = Queued { at: now, event };
        let added = queued_bytes(&queued);
        inner.queue.push(queued);
        inner.queue_bytes += added;
        if inner.queue_bytes > QUEUE_MOST_BYTES {
            // the oldest go, and the file is written again
            let mut dropped = 0;
            while inner.queue_bytes > QUEUE_MOST_BYTES * 9 / 10 && dropped < inner.queue.len() {
                inner.queue_bytes -= queued_bytes(&inner.queue[dropped]);
                dropped += 1;
            }
            inner.queue.drain(..dropped);
            shared.persist(&mut inner);
        } else if let Some(last) = inner.queue.last() {
            // one line more: the window's thread does not write the whole queue for each event
            shared.append(last);
        }
        if inner.queue.len() >= BATCH_WHEN {
            inner.wake = true;
            shared.wake.notify_all();
        }
    }

    /// The events waiting.
    pub fn queued(&self) -> Vec<Event> {
        self.shared.as_ref().map_or_else(Vec::new, |s| {
            s.lock().queue.iter().map(|q| q.event.clone()).collect()
        })
    }

    /// The next batch, as it would be sent (the install id is shown as `(made when the first report is sent)`
    /// until there is one): what *What is sent* shows.
    pub fn preview(&self) -> String {
        let Some(shared) = &self.shared else {
            return "This build of QuickAI sends no telemetry.".to_string();
        };
        let inner = shared.lock();
        let install = read_install_id(&shared.dir)
            .unwrap_or_else(|| "(made when the first report is sent)".to_string());
        let level = shared.level_of(&inner);
        let events: Vec<Event> = inner
            .queue
            .iter()
            .take(BATCH_MOST)
            .map(|q| q.event.clone())
            .collect();
        let batch = Batch::new(
            "(made when it is sent)".to_string(),
            shared.identity.common(&install, &shared.session, level),
            events,
        );
        serde_json::to_string_pretty(&batch).unwrap_or_default()
    }

    /// What was sent, as it was sent: the telemetry log.
    pub fn log_text(&self) -> String {
        self.shared
            .as_ref()
            .and_then(|s| std::fs::read_to_string(s.dir.join("telemetry.log")).ok())
            .unwrap_or_default()
    }

    /// The install id, if one was made.
    pub fn install_id(&self) -> Option<String> {
        self.shared.as_ref().and_then(|s| read_install_id(&s.dir))
    }

    /// Forgets the install id: the next report makes a new one, not tied to what was sent before.
    pub fn new_install_id(&self) {
        if let Some(shared) = &self.shared {
            let _ = std::fs::remove_file(shared.dir.join("install-id"));
        }
    }

    /// What EverybodyAI holds for this install (`GET /v1/me`), as JSON text.
    pub fn download_held(&self) -> Result<String, String> {
        let shared = self
            .shared
            .as_ref()
            .ok_or("This build of QuickAI sends no telemetry.")?;
        let install = read_install_id(&shared.dir).ok_or(
            "Nothing was ever sent from this install, so EverybodyAI holds nothing for it.",
        )?;
        send::me(&shared.endpoint, &install, false, SEND_TIMEOUT)
    }

    /// Deletes what EverybodyAI holds for this install (`DELETE /v1/me`), then forgets the install id and empties
    /// the queue, so that nothing sent later is tied to what was deleted. The answer is the server's receipt.
    pub fn delete_held(&self) -> Result<String, String> {
        let shared = self
            .shared
            .as_ref()
            .ok_or("This build of QuickAI sends no telemetry.")?;
        let install = read_install_id(&shared.dir).ok_or(
            "Nothing was ever sent from this install, so EverybodyAI holds nothing for it.",
        )?;
        let receipt = send::me(&shared.endpoint, &install, true, SEND_TIMEOUT)?;
        let _ = std::fs::remove_file(shared.dir.join("install-id"));
        let mut inner = shared.lock();
        inner.queue.clear();
        shared.persist(&mut inner);
        Ok(receipt)
    }

    /// What the window keeps up to date for a crash report.
    pub fn context(&self) -> Arc<Mutex<CrashContext>> {
        self.shared.as_ref().map_or_else(
            || Arc::new(Mutex::new(CrashContext::default())),
            |s| s.context.clone(),
        )
    }

    /// Installs the panic hook that writes a crash report to `crashes/` (after which the hook that was there runs).
    /// It is written whatever the level: it stays on this computer until it is sent or deleted.
    pub fn install_panic_hook(&self, cleaner: Cleaner) {
        let Some(shared) = &self.shared else { return };
        let hook = crash::Hook {
            dir: shared.dir.join("crashes"),
            common: shared.identity.common("", &shared.session, Level::Off),
            cleaner,
            context: shared.context.clone(),
            started: shared.started,
            id: Box::new(random_id),
            clock: shared.clock.clone(),
        };
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            hook.record(info);
            previous(info);
        }));
    }

    /// The crash reports not sent and not set aside, the newest first.
    pub fn pending_crashes(&self) -> Vec<PathBuf> {
        let Some(shared) = &self.shared else {
            return Vec::new();
        };
        let mut files: Vec<(std::time::SystemTime, PathBuf)> =
            std::fs::read_dir(shared.dir.join("crashes"))
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "json"))
                .map(|p| {
                    (
                        std::fs::metadata(&p)
                            .and_then(|m| m.modified())
                            .unwrap_or(std::time::UNIX_EPOCH),
                        p,
                    )
                })
                .collect();
        files.sort_by_key(|f| std::cmp::Reverse(f.0));
        files.into_iter().map(|(_, p)| p).collect()
    }

    /// The report a crash file holds.
    pub fn read_crash(path: &Path) -> Option<Batch> {
        serde_json::from_str(&std::fs::read_to_string(path).ok()?).ok()
    }

    /// Sends one crash report now (`POST /v1/crash`), with this install's id (made if there is none) and the level in
    /// force; sent or refused, the file goes. This is how a crash is sent when the person says so at Off.
    pub fn send_crash(&self, path: &Path) -> Result<(), String> {
        self.send_crash_within(path, SEND_TIMEOUT)
    }

    fn send_crash_within(&self, path: &Path, timeout: Duration) -> Result<(), String> {
        let shared = self
            .shared
            .as_ref()
            .ok_or("This build of QuickAI sends no telemetry.")?;
        if shared.caps.iter().any(|c| c.level == Level::Off) {
            return Err("Telemetry is turned off for this installation.".to_string());
        }
        let mut batch = Self::read_crash(path).ok_or("The crash report could not be read.")?;
        batch.common.install = ensure_install_id(&shared.dir);
        batch.common.level = shared.level_of(&shared.lock()).id().to_string();
        schema::check_batch(&batch)?;
        let json = serde_json::to_string(&batch).map_err(|e| e.to_string())?;
        let answer = send::post(
            &shared.endpoint,
            "/v1/crash",
            &batch.common.install,
            &json,
            timeout,
        );
        shared.log("/v1/crash", &batch, answer);
        match answer {
            Answer::Sent | Answer::Refused => {
                let _ = std::fs::remove_file(path);
                Ok(())
            }
            Answer::Blocked => {
                Err("EverybodyAI's server does not take reports from this install now.".to_string())
            }
            Answer::Retired => {
                Err("This version of QuickAI no longer sends reports; an update does.".to_string())
            }
            Answer::Later(_) => Err(
                "EverybodyAI's server could not be reached. The report is kept; try again later."
                    .to_string(),
            ),
        }
    }

    /// Sets a crash report aside, unsent: it stays in `crashes/kept` (for a GitHub issue) until it is 90 days old.
    pub fn keep_crash(&self, path: &Path) {
        let Some(shared) = &self.shared else { return };
        let kept = shared.dir.join("crashes").join("kept");
        let _ = std::fs::create_dir_all(&kept);
        if let Some(name) = path.file_name() {
            let _ = std::fs::rename(path, kept.join(name));
        }
    }

    /// One round of sending: the crash reports waiting, if the level allows crashes, then a batch of events if one
    /// is due (or, with `forced`, whatever waits). The answer of the batch, if one was sent.
    pub fn send_due(&self, forced: bool, timeout: Duration) -> Option<Answer> {
        let shared = self.shared.as_ref()?;
        shared.send_crashes(self, timeout);
        shared.send_batch(forced, timeout)
    }

    /// Starts the thread that sends: it wakes every minute, when many events wait, and when the level changes.
    pub fn spawn_sender(&self) {
        let Some(shared) = self.shared.clone() else {
            return;
        };
        {
            let mut inner = shared.lock();
            if inner.sender {
                return;
            }
            inner.sender = true;
        }
        let telemetry = self.clone();
        let _ = std::thread::Builder::new()
            .name("telemetry".into())
            .spawn(move || loop {
                telemetry.send_due(false, SEND_TIMEOUT);
                let inner = shared.lock();
                let (mut inner, _) = shared
                    .wake
                    .wait_timeout_while(inner, Duration::from_secs(60), |i| !i.wake && !i.closing)
                    .unwrap_or_else(|e| e.into_inner());
                if inner.closing {
                    return;
                }
                inner.wake = false;
            });
    }

    /// The app is closing: whatever waits is sent, with a short timeout, the sender stops, and this session's marker
    /// goes (so the next start does not count it as an unclean exit).
    pub fn end_session(&self) {
        let Some(shared) = &self.shared else { return };
        {
            let mut inner = shared.lock();
            inner.closing = true;
            shared.wake.notify_all();
        }
        shared.send_batch(true, CLOSING_TIMEOUT);
        let _ = std::fs::remove_file(
            shared
                .dir
                .join(format!("running-{}.json", std::process::id())),
        );
    }

    /// The state kept between starts (for the tests and Settings).
    pub fn send_state(&self) -> SendState {
        self.shared
            .as_ref()
            .map(|s| s.lock().state.clone())
            .unwrap_or_default()
    }

    /// The common properties as they would be sent now.
    pub fn identity(&self) -> Option<&Identity> {
        self.shared.as_ref().map(|s| &s.identity)
    }
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    fn level_of(&self, inner: &Inner) -> Level {
        let caps: Vec<Level> = self.caps.iter().map(|c| c.level).collect();
        level::in_force(inner.chosen, &caps)
    }

    fn halted(&self, state: &SendState, now: u64) -> bool {
        state.blocked_until > now
            || state.retry_at > now
            || state.retired_version.as_deref() == Some(self.identity.app.version.as_str())
    }

    /// Sends the crash reports waiting, when the level allows crashes, until one is not taken.
    fn send_crashes(&self, telemetry: &Telemetry, timeout: Duration) {
        let now = (self.clock)();
        {
            let inner = self.lock();
            if !self.level_of(&inner).allows(Level::Crash) || self.halted(&inner.state, now) {
                return;
            }
        }
        for path in telemetry.pending_crashes() {
            if telemetry.send_crash_within(&path, timeout).is_err() {
                let mut inner = self.lock();
                inner.state.failures += 1;
                inner.state.retry_at =
                    now + WAITS[(inner.state.failures as usize - 1).min(WAITS.len() - 1)];
                self.persist(&mut inner);
                return;
            }
        }
    }

    /// Sends one batch of the events waiting, if one is due.
    fn send_batch(&self, forced: bool, timeout: Duration) -> Option<Answer> {
        let now = (self.clock)();
        let (json, batch, sent_at) = {
            let mut inner = self.lock();
            let level = self.level_of(&inner);
            if level == Level::Off || inner.queue.is_empty() {
                return None;
            }
            // what waited too long while the server could not be reached goes
            if inner.state.failures > 0 {
                let before = inner.queue.len();
                inner
                    .queue
                    .retain(|q| now.saturating_sub(q.at) <= GIVE_UP_AFTER);
                if inner.queue.len() != before {
                    self.persist(&mut inner);
                }
            }
            // blocked, retired or waiting after a failure: not even the last batch of a session goes
            if inner.queue.is_empty() || self.halted(&inner.state, now) {
                return None;
            }
            let settled = now >= self.started + FIRST_SEND_AFTER;
            let due = forced
                || (settled
                    && (inner.queue.len() >= BATCH_WHEN
                        || now.saturating_sub(inner.state.last_sent) >= BATCH_EVERY));
            if !due {
                return None;
            }
            let install = ensure_install_id(&self.dir);
            let common = self.identity.common(&install, &self.session, level);
            let mut count = inner.queue.len().min(BATCH_MOST);
            let (json, batch) = loop {
                let events = inner.queue[..count]
                    .iter()
                    .map(|q| q.event.clone())
                    .collect();
                let batch = Batch::new(random_id(), common.clone(), events);
                let json = serde_json::to_string(&batch).unwrap_or_default();
                if json.len() <= BATCH_MOST_BYTES || count == 1 {
                    break (json, batch);
                }
                count /= 2;
            };
            let sent_at: Vec<u64> = inner.queue[..count].iter().map(|q| q.at).collect();
            (json, batch, sent_at)
        };
        let answer = send::post(
            &self.endpoint,
            "/v1/events",
            &batch.common.install,
            &json,
            timeout,
        );
        self.log("/v1/events", &batch, answer);
        let mut inner = self.lock();
        let drop_sent = |inner: &mut Inner| {
            // the events of the batch, by their moments and contents: the queue may have changed meanwhile
            for (at, event) in sent_at.iter().zip(&batch.events) {
                if let Some(i) = inner
                    .queue
                    .iter()
                    .position(|q| q.at == *at && q.event == *event)
                {
                    inner.queue.remove(i);
                }
            }
        };
        match answer {
            Answer::Sent => {
                drop_sent(&mut inner);
                inner.state.last_sent = now;
                inner.state.failures = 0;
                inner.state.retry_at = 0;
            }
            Answer::Refused => drop_sent(&mut inner),
            Answer::Blocked => inner.state.blocked_until = now + BLOCKED_FOR,
            Answer::Retired => {
                inner.state.retired_version = Some(self.identity.app.version.clone())
            }
            Answer::Later(after) => {
                inner.state.failures += 1;
                let wait = WAITS[(inner.state.failures as usize - 1).min(WAITS.len() - 1)];
                inner.state.retry_at = now + after.unwrap_or(wait);
            }
        }
        self.persist(&mut inner);
        Some(answer)
    }

    /// One event more at the end of `queue.jsonl`.
    fn append(&self, queued: &Queued) {
        use std::io::Write;
        if let Ok(line) = serde_json::to_string(queued) {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(self.dir.join("queue.jsonl"));
            if let Ok(mut file) = file {
                let _ = file.write_all(format!("{line}\n").as_bytes());
            }
        }
    }

    /// Writes the queue and the state.
    fn persist(&self, inner: &mut Inner) {
        inner.queue_bytes = inner.queue.iter().map(queued_bytes).sum();
        let lines: String = inner
            .queue
            .iter()
            .filter_map(|q| serde_json::to_string(q).ok())
            .map(|line| line + "\n")
            .collect();
        write_whole(&self.dir.join("queue.jsonl"), &lines);
        if let Ok(json) = serde_json::to_string_pretty(&inner.state) {
            write_whole(&self.dir.join("state.json"), &json);
        }
    }

    /// Appends what was sent to the telemetry log, keeping its last `LOG_MOST_BYTES`.
    fn log(&self, path: &str, batch: &Batch, answer: Answer) {
        let file = self.dir.join("telemetry.log");
        let mut text = std::fs::read_to_string(&file).unwrap_or_default();
        let outcome = match answer {
            Answer::Sent => "taken".to_string(),
            Answer::Refused => "refused by the server, dropped".to_string(),
            Answer::Blocked => "not taken: this install is blocked".to_string(),
            Answer::Retired => "not taken: this version is no longer collected".to_string(),
            Answer::Later(_) => "not reached: kept, to try again".to_string(),
        };
        text.push_str(&format!(
            "=== {}  POST {path}  {} event(s)  {outcome}\n{}\n\n",
            measure::moment_utc((self.clock)()),
            batch.events.len(),
            serde_json::to_string_pretty(batch).unwrap_or_default()
        ));
        if text.len() > LOG_MOST_BYTES {
            let mut cut = text.len() - LOG_MOST_BYTES;
            while !text.is_char_boundary(cut) {
                cut += 1;
            }
            // from the start of an entry
            let start = text[cut..].find("=== ").map_or(cut, |i| cut + i);
            text = text[start..].to_string();
        }
        write_whole(&file, &text);
    }
}

fn queued_bytes(q: &Queued) -> usize {
    serde_json::to_string(q).map_or(0, |s| s.len() + 1)
}

fn write_whole(path: &Path, text: &str) {
    let mut part = path.as_os_str().to_os_string();
    part.push(".part");
    let part = PathBuf::from(part);
    if std::fs::write(&part, text).is_ok() {
        let _ = std::fs::rename(&part, path);
    }
}

fn read_queue(dir: &Path) -> Vec<Queued> {
    std::fs::read_to_string(dir.join("queue.jsonl"))
        .unwrap_or_default()
        .lines()
        .filter_map(|line| serde_json::from_str::<Queued>(line).ok())
        .filter(|q| schema::check(&q.event).is_ok())
        .collect()
}

fn read_install_id(dir: &Path) -> Option<String> {
    std::fs::read_to_string(dir.join("install-id"))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| s.len() == 36 && measure::is_plain_name(s))
}

/// The install id, made (and kept, readable by the person only) if there is none.
fn ensure_install_id(dir: &Path) -> String {
    if let Some(id) = read_install_id(dir) {
        return id;
    }
    let id = random_id();
    let path = dir.join("install-id");
    let _ = std::fs::create_dir_all(dir);
    write_whole(&path, &id);
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
    }
    id
}

/// Deletes crash files (sent or set aside) older than `CRASHES_KEPT_FOR`.
fn prune_crashes(dir: &Path, now: u64) {
    for folder in [dir.join("crashes"), dir.join("crashes").join("kept")] {
        for entry in std::fs::read_dir(&folder).into_iter().flatten().flatten() {
            let path = entry.path();
            let age = std::fs::metadata(&path)
                .and_then(|m| m.modified())
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| now.saturating_sub(d.as_secs()));
            if path.is_file() && age.is_some_and(|a| a > CRASHES_KEPT_FOR) {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The sessions whose marker is left and whose program is gone, which no crash report names: they ended without
/// closing. Their markers are taken away.
fn ended_uncleanly(dir: &Path) -> usize {
    let crashed: Vec<String> = [dir.join("crashes"), dir.join("crashes").join("kept")]
        .iter()
        .flat_map(|d| std::fs::read_dir(d).into_iter().flatten().flatten())
        .filter_map(|e| Telemetry::read_crash(&e.path()))
        .map(|b| b.common.session)
        .collect();
    let mut unclean = 0;
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default()
            .to_string();
        if !(name.starts_with("running-") && name.ends_with(".json")) {
            continue;
        }
        let Some(marker) = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Marker>(&s).ok())
        else {
            let _ = std::fs::remove_file(&path);
            continue;
        };
        if marker.pid == std::process::id() || program_alive(marker.pid) {
            continue;
        }
        if !crashed.contains(&marker.session) {
            unclean += 1;
        }
        let _ = std::fs::remove_file(&path);
    }
    unclean
}

/// Whether a QuickAI program with this pid is running.
fn program_alive(pid: u32) -> bool {
    use sysinfo::{Pid, ProcessesToUpdate, System};
    let pid = Pid::from_u32(pid);
    let mut system = System::new();
    system.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    system.process(pid).is_some_and(|p| {
        p.name()
            .to_string_lossy()
            .to_ascii_lowercase()
            .contains("quickai")
    })
}

#[cfg(test)]
mod tests;
