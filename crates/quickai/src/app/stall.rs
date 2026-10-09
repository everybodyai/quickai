//! A record of the times the window stopped answering (docs/comfyui.md, "If the window stops answering").
//!
//! A window drawn by one thread stops answering when that thread waits for something that does not come:
//! the disk, the display server, a lock. While something runs that the window is showing (an install, a
//! download), a thread of its own looks at the window once a second. If no frame has been drawn for a few
//! seconds it writes down what the window's thread was waiting in and how the machine's memory and disk
//! were doing, and later when the window answered again, so that a freeze that cannot be seen again on
//! purpose still says what it was. Nothing is sent anywhere: the record is a file in the data directory.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use eframe::egui;

use crate::hardware::format_duration;

/// How long without a frame is a stall.
pub const LIMIT: Duration = Duration::from_secs(5);
/// How often the watcher looks.
pub const TICK: Duration = Duration::from_secs(1);
/// How big the record may grow before its older half is dropped.
const MAX_RECORD: u64 = 200_000;

struct Shared {
    began: Instant,
    /// When the last frame was drawn, in milliseconds since `began`.
    last_beat: AtomicU64,
    /// What the window is showing that is running, if anything: nothing is expected of an idle window,
    /// which draws no frames at all.
    running: Mutex<Option<&'static str>>,
    /// Set when the application is gone and the watcher should end.
    gone: AtomicBool,
    /// What the watcher has to say to the person, for the log pane.
    notices: Mutex<Vec<String>>,
}

/// The pulse of the window: each frame beats, and a watcher looks at how long ago.
pub struct Heartbeat {
    shared: Arc<Shared>,
}

impl Heartbeat {
    pub fn new() -> Heartbeat {
        Heartbeat {
            shared: Arc::new(Shared {
                began: Instant::now(),
                last_beat: AtomicU64::new(0),
                running: Mutex::new(None),
                gone: AtomicBool::new(false),
                notices: Mutex::new(Vec::new()),
            }),
        }
    }

    /// A frame is being drawn. `running` says what the window is showing that goes on without it
    /// ("an install"), or `None`.
    pub fn beat(&self, running: Option<&'static str>) {
        self.shared.last_beat.store(
            self.shared.began.elapsed().as_millis() as u64,
            Ordering::Relaxed,
        );
        if let Ok(mut now) = self.shared.running.lock() {
            *now = running;
        }
    }

    /// What the watcher has said since it was last asked: for the log pane.
    pub fn take_notices(&self) -> Vec<String> {
        self.shared
            .notices
            .lock()
            .map(|mut n| std::mem::take(&mut *n))
            .unwrap_or_default()
    }

    /// Starts the thread that looks, and writes to `record`. `thread` is the id of the thread that
    /// draws the window (Linux), whose state is part of what is written down.
    pub fn watch(
        &self,
        ctx: egui::Context,
        record: PathBuf,
        thread: Option<u32>,
        limit: Duration,
        tick: Duration,
    ) {
        let shared = Arc::clone(&self.shared);
        std::thread::spawn(move || look(&shared, &ctx, &record, thread, limit, tick));
    }

    #[cfg(test)]
    fn weak(&self) -> std::sync::Weak<Shared> {
        Arc::downgrade(&self.shared)
    }
}

impl Default for Heartbeat {
    fn default() -> Self {
        Heartbeat::new()
    }
}

impl Drop for Heartbeat {
    fn drop(&mut self) {
        self.shared.gone.store(true, Ordering::Relaxed);
    }
}

fn look(
    shared: &Shared,
    ctx: &egui::Context,
    record: &Path,
    thread: Option<u32>,
    limit: Duration,
    tick: Duration,
) {
    // when the last frame before the stall was drawn
    let mut stalled_from: Option<Instant> = None;
    // the last time the window was seen covered or minimized
    let mut hidden_at: Option<Instant> = None;
    while !shared.gone.load(Ordering::Relaxed) {
        std::thread::sleep(tick);
        let running = shared.running.lock().ok().and_then(|r| *r);
        let Some(running) = running else {
            stalled_from = None;
            hidden_at = None;
            continue;
        };
        // a window that is covered or minimized is not drawn (the application is not asked to, which
        // is how a window that nobody can see costs nothing): that is no stall
        if ctx.input(|input| input.viewport().visible()) == Some(false) {
            hidden_at = Some(Instant::now());
            stalled_from = None;
            continue;
        }
        // a window that is alive and has nothing to say draws nothing: ask it for a frame, so that the
        // silence of one that is not is the silence of a window that cannot
        ctx.request_repaint();
        let mut since_beat = Duration::from_millis(
            (shared.began.elapsed().as_millis() as u64)
                .saturating_sub(shared.last_beat.load(Ordering::Relaxed)),
        );
        // a window that was hidden has had no chance to draw since it was shown
        if let Some(hidden) = hidden_at {
            since_beat = since_beat.min(hidden.elapsed());
        }
        match stalled_from {
            None if since_beat >= limit => {
                stalled_from = Some(Instant::now() - since_beat);
                append(
                    record,
                    &format!(
                        "{} · the window has not answered for {} while {running} was running\n{}",
                        utc(now_epoch()),
                        format_duration(since_beat),
                        what_the_machine_says(thread)
                    ),
                );
            }
            Some(from) if since_beat < limit => {
                let stalled = from.elapsed().saturating_sub(since_beat);
                append(
                    record,
                    &format!(
                        "{} · the window answers again after {}\n",
                        utc(now_epoch()),
                        format_duration(stalled)
                    ),
                );
                if let Ok(mut notices) = shared.notices.lock() {
                    notices.push(format!(
                        "[quickai] The window did not answer for {} while {running} was running. What the machine was doing is written in {}.",
                        format_duration(stalled),
                        record.display()
                    ));
                }
                stalled_from = None;
            }
            _ => {}
        }
    }
}

fn now_epoch() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `2026-10-08 01:06:12 UTC`
fn utc(epoch: u64) -> String {
    let (days, rest) = (epoch / 86_400, epoch % 86_400);
    // civil date from days since 1970-01-01 (H. Hinnant's algorithm)
    let z = days as i64 + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_index = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_index + 2) / 5 + 1;
    let month = if month_index < 10 {
        month_index + 3
    } else {
        month_index - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Adds `text` to the end of the record, keeping the file small: past its limit the older half goes.
fn append(record: &Path, text: &str) {
    use std::io::Write;
    if let Some(dir) = record.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if std::fs::metadata(record).is_ok_and(|m| m.len() > MAX_RECORD) {
        if let Ok(old) = std::fs::read_to_string(record) {
            // from the first line that starts after the middle (a line holds characters of more than
            // one byte, so the middle is not always a place a string can be cut)
            let from = old.len() / 2;
            let start = old.as_bytes()[from..]
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(old.len(), |n| from + n + 1);
            let _ = std::fs::write(record, &old[start..]);
        }
    }
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(record)
    {
        let _ = file.write_all(text.as_bytes());
    }
}

/// What the machine says about the window's thread and about itself, as lines of the record. Where the
/// system does not say (anything but Linux), only that.
fn what_the_machine_says(thread: Option<u32>) -> String {
    #[cfg(target_os = "linux")]
    {
        linux::describe(thread)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = thread;
        "  (this system does not say what the window's thread was doing)\n".to_string()
    }
}

#[cfg(target_os = "linux")]
mod linux {
    use crate::hardware::format_bytes;

    fn read(path: &str) -> String {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// The name of a system call by its number on x86-64, for the ones a window waits in.
    fn syscall_name(number: i64) -> Option<&'static str> {
        Some(match number {
            0 => "read",
            1 => "write",
            7 => "poll",
            16 => "ioctl",
            17 => "pread64",
            18 => "pwrite64",
            23 => "select",
            35 => "nanosleep",
            46 => "sendmsg",
            47 => "recvmsg",
            74 => "fsync",
            202 => "futex",
            217 => "getdents64",
            230 => "clock_nanosleep",
            232 => "epoll_wait",
            257 => "openat",
            262 => "newfstatat",
            270 => "pselect6",
            271 => "ppoll",
            281 => "epoll_pwait",
            332 => "statx",
            _ => return None,
        })
    }

    fn state_word(state: &str) -> &'static str {
        match state {
            "R" => "running",
            "S" => "sleeping",
            "D" => "waiting on the disk or a device, which nothing can interrupt",
            "T" | "t" => "stopped",
            "Z" => "ended",
            "I" => "idle",
            _ => "in a state the system does not name",
        }
    }

    /// `kB` lines of `/proc/<..>/status` and `/proc/meminfo` as bytes.
    fn kib(text: &str, key: &str) -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        let number: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(number * 1024)
    }

    /// The share of the last ten seconds in which something waited (`some avg10=` of a pressure file).
    fn pressure(path: &str) -> Option<String> {
        let text = std::fs::read_to_string(path).ok()?;
        let line = text.lines().find(|l| l.starts_with("some"))?;
        let value = line
            .split_whitespace()
            .find_map(|w| w.strip_prefix("avg10="))?;
        Some(format!("{value}%"))
    }

    pub fn describe(thread: Option<u32>) -> String {
        let mut text = String::new();
        if let Some(tid) = thread {
            let base = format!("/proc/self/task/{tid}");
            let stat = read(&format!("{base}/stat"));
            let state = stat
                .rsplit_once(')')
                .and_then(|(_, rest)| rest.split_whitespace().next())
                .unwrap_or("?");
            let waiting = read(&format!("{base}/wchan"));
            let call = read(&format!("{base}/syscall"));
            let number = call
                .split_whitespace()
                .next()
                .and_then(|n| n.parse::<i64>().ok());
            let call_text = match (number, call.as_str()) {
                (_, "running") => "none (it was running)".to_string(),
                (Some(n), _) => match syscall_name(n) {
                    Some(name) => format!("{name} ({n})"),
                    None => format!("number {n}"),
                },
                _ => "not said".to_string(),
            };
            text.push_str(&format!(
                "  its thread: {state}, {}; waiting in the kernel at {}; system call: {call_text}\n",
                state_word(state),
                if waiting.is_empty() || waiting == "0" {
                    "nothing it names"
                } else {
                    &waiting
                }
            ));
        }
        let meminfo = read("/proc/meminfo");
        let status = read("/proc/self/status");
        let size = |value: Option<u64>| value.map_or("?".to_string(), format_bytes);
        text.push_str(&format!(
            "  memory: {} available, {} of {} swap free; this program holds {} and has {} in swap\n",
            size(kib(&meminfo, "MemAvailable:")),
            size(kib(&meminfo, "SwapFree:")),
            size(kib(&meminfo, "SwapTotal:")),
            size(kib(&status, "VmRSS:")),
            size(kib(&status, "VmSwap:")),
        ));
        text.push_str(&format!(
            "  the machine in the last 10 s: something waited for memory {} of the time, for the disk {}\n",
            pressure("/proc/pressure/memory").unwrap_or_else(|| "?".into()),
            pressure("/proc/pressure/io").unwrap_or_else(|| "?".into()),
        ));
        text
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testkit::{scratch, wait_until};

    fn this_thread() -> Option<u32> {
        #[cfg(target_os = "linux")]
        {
            // SAFETY: gettid takes no arguments and cannot fail.
            Some(unsafe { libc::gettid() } as u32)
        }
        #[cfg(not(target_os = "linux"))]
        {
            None
        }
    }

    #[test]
    fn p3_f82_a_window_that_stops_answering_is_written_down_with_what_its_thread_was_doing() {
        let dir = scratch("f82");
        let record = dir.join("ui-stalls.log");
        let heartbeat = Heartbeat::new();
        heartbeat.watch(
            egui::Context::default(),
            record.clone(),
            this_thread(),
            Duration::from_millis(300),
            Duration::from_millis(40),
        );

        // frames are drawn, and nothing is written
        for _ in 0..8 {
            heartbeat.beat(Some("an install"));
            std::thread::sleep(Duration::from_millis(40));
        }
        assert!(
            !record.exists(),
            "a window that answers is not written about"
        );

        // and then no frame for longer than the limit
        wait_until(Duration::from_secs(5), "the stall to be written", || {
            std::fs::read_to_string(&record).is_ok_and(|t| t.contains("has not answered"))
        });
        let written = std::fs::read_to_string(&record).unwrap();
        assert!(
            written.contains("UTC · the window has not answered for")
                && written.contains("while an install was running"),
            "{written}"
        );
        #[cfg(target_os = "linux")]
        {
            assert!(written.contains("  its thread: "), "{written}");
            assert!(
                written.contains("  memory: ") && written.contains("swap free"),
                "{written}"
            );
            assert!(written.contains("something waited for memory"), "{written}");
        }
        assert!(
            heartbeat.take_notices().is_empty(),
            "the person is told when the window is back, since until then there is no one to tell"
        );

        // the window answers again: that is written, and the person is told in the log
        heartbeat.beat(Some("an install"));
        wait_until(Duration::from_secs(5), "the end of the stall", || {
            std::fs::read_to_string(&record).is_ok_and(|t| t.contains("answers again after"))
        });
        let notices = heartbeat.take_notices();
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(
            notices[0].contains("did not answer for") && notices[0].contains("ui-stalls.log"),
            "{notices:?}"
        );
        assert!(heartbeat.take_notices().is_empty(), "said once");

        // the watcher ends with the application
        let weak = heartbeat.weak();
        drop(heartbeat);
        wait_until(Duration::from_secs(5), "the watcher to end", || {
            weak.upgrade().is_none()
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f82c_a_window_that_is_covered_or_minimized_is_not_drawn_and_that_is_no_stall() {
        let dir = scratch("f82c");
        let record = dir.join("ui-stalls.log");
        let ctx = egui::Context::default();
        let show = |covered: bool| {
            let mut raw = egui::RawInput::default();
            raw.viewports.insert(
                egui::ViewportId::ROOT,
                egui::ViewportInfo {
                    occluded: Some(covered),
                    minimized: Some(false),
                    ..Default::default()
                },
            );
            let mut output = ctx.run_ui(raw, |_| {});
            output.textures_delta.clear();
        };
        show(true);
        assert_eq!(
            ctx.input(|i| i.viewport().visible()),
            Some(false),
            "the context says the window is covered"
        );
        let heartbeat = Heartbeat::new();
        heartbeat.watch(
            ctx.clone(),
            record.clone(),
            this_thread(),
            Duration::from_millis(400),
            Duration::from_millis(40),
        );
        heartbeat.beat(Some("an install"));
        std::thread::sleep(Duration::from_millis(1000));
        assert!(
            !record.exists(),
            "nothing is expected of a window nobody can see"
        );

        // it is shown again: it has had no chance to draw yet, and that is no stall either
        show(false);
        std::thread::sleep(Duration::from_millis(100));
        assert!(!record.exists(), "just shown");
        // but a window that is shown and silent is one
        wait_until(Duration::from_secs(5), "the stall to be written", || {
            record.exists()
        });
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn p3_f82b_a_window_with_nothing_running_is_not_expected_to_draw() {
        let dir = scratch("f82b");
        let record = dir.join("ui-stalls.log");
        let heartbeat = Heartbeat::new();
        heartbeat.watch(
            egui::Context::default(),
            record.clone(),
            this_thread(),
            Duration::from_millis(100),
            Duration::from_millis(30),
        );
        heartbeat.beat(None);
        std::thread::sleep(Duration::from_millis(600));
        assert!(
            !record.exists(),
            "an idle window draws no frames, and that is no stall"
        );
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn the_record_is_in_utc_and_stays_small() {
        assert_eq!(utc(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00:00 UTC", "a leap day");
        assert_eq!(utc(1_700_000_000), "2023-11-14 22:13:20 UTC");
        assert_eq!(utc(1_791_421_572), "2026-10-08 01:06:12 UTC");

        let dir = scratch("record");
        let record = dir.join("log");
        let line = format!("{}\n", "x".repeat(99));
        for _ in 0..3_000 {
            append(&record, &line);
        }
        let size = std::fs::metadata(&record).unwrap().len();
        assert!(size < MAX_RECORD + 1_000, "{size}");
        // what is kept is whole lines, the newest
        let kept = std::fs::read_to_string(&record).unwrap();
        assert!(kept.lines().all(|l| l.len() == 99));
        let _ = std::fs::remove_dir_all(&dir);

        // and a line with characters of more than one byte is not cut in the middle of one, wherever the middle is
        for width in 90..100 {
            let record = dir.join(format!("utf8-{width}"));
            let line = format!("{}\n", "·".repeat(width));
            for _ in 0..(MAX_RECORD as usize / line.len() + 40) {
                append(&record, &line);
            }
            let kept = std::fs::read_to_string(&record).unwrap();
            assert!(
                kept.lines().all(|l| l == line.trim_end()),
                "width {width}: a line was cut"
            );
        }
        let _ = std::fs::remove_dir_all(dir);
    }
}
