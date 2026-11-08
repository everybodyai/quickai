//! What the machine says of a moment: how its memory and its swap were, how much of the last ten seconds anything
//! waited for memory or for the disk, how much QuickAI held, and what one thread of QuickAI was doing (its state, the
//! kernel function it waited in, its system call). The stall watcher writes it down for the report of a stall; the
//! panic hook puts the memory and the waits in the report of a crash. Linux says all of it; the other systems say
//! nothing here, and then a report says nothing of it.

use quickai_core::report::{measure, Report};

/// The moment, as read. Everything is `None` that the system did not say.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MachineNow {
    pub memory_available: Option<u64>,
    pub swap_free: Option<u64>,
    pub swap_total: Option<u64>,
    /// What QuickAI holds in memory, and has in swap.
    pub resident: Option<u64>,
    pub swapped: Option<u64>,
    /// The share of the last ten seconds in which something waited, in percent (`some avg10=`).
    pub memory_pressure: Option<f64>,
    pub io_pressure: Option<f64>,
    pub thread: Option<ThreadNow>,
}

/// A thread of QuickAI at that moment.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ThreadNow {
    /// The letter of `/proc/<pid>/task/<tid>/stat` (`S`, `D`).
    pub state: String,
    /// The kernel function it waits in, when it names one.
    pub waiting_in: Option<String>,
    pub syscall: Syscall,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Syscall {
    /// It was running, in no system call.
    Running,
    /// The number of the call it was in.
    Number(i64),
    /// The system did not say.
    Unknown,
}

impl Syscall {
    /// Its name, when it is one a window waits in (x86-64 numbering), else `None`.
    pub fn name(&self) -> Option<&'static str> {
        match self {
            Syscall::Number(n) => syscall_name(*n),
            _ => None,
        }
    }

    /// As a report says it: the name, the number when it has no name here, `none` when it was running.
    pub fn said(&self) -> Option<String> {
        match self {
            Syscall::Running => Some("none".to_string()),
            Syscall::Number(n) => {
                Some(syscall_name(*n).map_or_else(|| n.to_string(), str::to_string))
            }
            Syscall::Unknown => None,
        }
    }
}

/// The name of a system call by its number on x86-64, for the ones a window waits in.
fn syscall_name(number: i64) -> Option<&'static str> {
    if !cfg!(target_arch = "x86_64") {
        return None;
    }
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
        75 => "fdatasync",
        76 => "truncate",
        77 => "ftruncate",
        82 => "rename",
        87 => "unlink",
        202 => "futex",
        217 => "getdents64",
        230 => "clock_nanosleep",
        232 => "epoll_wait",
        257 => "openat",
        262 => "newfstatat",
        270 => "pselect6",
        271 => "ppoll",
        277 => "sync_file_range",
        281 => "epoll_pwait",
        316 => "renameat2",
        332 => "statx",
        _ => return None,
    })
}

/// What the machine says now; `thread` is the id of a thread of this program whose state is wanted (Linux).
pub fn now(thread: Option<u32>) -> MachineNow {
    #[cfg(target_os = "linux")]
    {
        linux::read(thread)
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = thread;
        MachineNow::default()
    }
}

/// `report` with what `machine` says of the memory and the waits, in ranges; with what its thread was doing too when
/// `thread` (a stall). What the machine did not say is left out.
pub fn with_machine(mut report: Report, machine: &MachineNow, thread: bool) -> Report {
    if let Some(bytes) = machine.memory_available {
        report = report.detail("Memory available (GB)", measure::available_bucket(bytes));
    }
    if let (Some(free), Some(total)) = (machine.swap_free, machine.swap_total) {
        report = report.detail("Swap free", measure::swap_free_bucket(free, total));
    }
    if let Some(p) = machine.memory_pressure {
        report = report.detail(
            "Waiting for memory (% of 10 s)",
            measure::pressure_bucket(p),
        );
    }
    if let Some(p) = machine.io_pressure {
        report = report.detail(
            "Waiting for the disk (% of 10 s)",
            measure::pressure_bucket(p),
        );
    }
    if let Some(bytes) = machine.resident {
        report = report.detail("QuickAI's memory", measure::app_memory_bucket(bytes));
    }
    if thread {
        if let Some(t) = &machine.thread {
            report = report.detail("Window thread", measure::thread_state(&t.state));
            if let Some(name) = t.waiting_in.as_deref().and_then(measure::kernel_function) {
                report = report.detail("Waiting in", name);
            }
            if let Some(call) = t.syscall.said() {
                report = report.detail("System call", call);
            }
        }
    }
    report
}

#[cfg(target_os = "linux")]
mod linux {
    use super::{MachineNow, Syscall, ThreadNow};

    fn read_file(path: &str) -> String {
        std::fs::read_to_string(path)
            .unwrap_or_default()
            .trim()
            .to_string()
    }

    /// A `kB` line of `/proc/<..>/status` or `/proc/meminfo`, as bytes.
    fn kib(text: &str, key: &str) -> Option<u64> {
        let line = text.lines().find(|l| l.starts_with(key))?;
        let number: u64 = line.split_whitespace().nth(1)?.parse().ok()?;
        Some(number * 1024)
    }

    /// `some avg10=` of a pressure file, in percent.
    fn pressure(path: &str) -> Option<f64> {
        let text = std::fs::read_to_string(path).ok()?;
        let line = text.lines().find(|l| l.starts_with("some"))?;
        line.split_whitespace()
            .find_map(|w| w.strip_prefix("avg10="))?
            .parse()
            .ok()
    }

    fn thread(tid: u32) -> Option<ThreadNow> {
        let base = format!("/proc/self/task/{tid}");
        let stat = read_file(&format!("{base}/stat"));
        let state = stat
            .rsplit_once(')')
            .and_then(|(_, rest)| rest.split_whitespace().next())?
            .to_string();
        let waiting = read_file(&format!("{base}/wchan"));
        let call = read_file(&format!("{base}/syscall"));
        let syscall = if call == "running" {
            Syscall::Running
        } else {
            call.split_whitespace()
                .next()
                .and_then(|n| n.parse::<i64>().ok())
                .map_or(Syscall::Unknown, Syscall::Number)
        };
        Some(ThreadNow {
            state,
            waiting_in: (!waiting.is_empty() && waiting != "0").then_some(waiting),
            syscall,
        })
    }

    pub fn read(tid: Option<u32>) -> MachineNow {
        let meminfo = read_file("/proc/meminfo");
        let status = read_file("/proc/self/status");
        MachineNow {
            memory_available: kib(&meminfo, "MemAvailable:"),
            swap_free: kib(&meminfo, "SwapFree:"),
            swap_total: kib(&meminfo, "SwapTotal:"),
            resident: kib(&status, "VmRSS:"),
            swapped: kib(&status, "VmSwap:"),
            memory_pressure: pressure("/proc/pressure/memory"),
            io_pressure: pressure("/proc/pressure/io"),
            thread: tid.and_then(thread),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_moment_becomes_lines_of_the_report_in_ranges() {
        use quickai_core::report::{Environment, Kind};
        let machine = MachineNow {
            memory_available: Some(3 * 1024 * 1024 * 1024),
            swap_free: Some(0),
            swap_total: Some(8 * 1024 * 1024 * 1024),
            resident: Some(300 * 1024 * 1024),
            swapped: Some(0),
            memory_pressure: Some(34.2),
            io_pressure: Some(61.0),
            thread: Some(ThreadNow {
                state: "D".into(),
                waiting_in: Some("folio_wait_bit_common".into()),
                syscall: Syscall::Number(0),
            }),
        };
        let report = || Report::new(Kind::WindowStalled, Environment::default());
        let stall = with_machine(report(), &machine, true);
        let line = |r: &Report, name: &str| {
            r.details
                .iter()
                .find(|(n, _)| n == name)
                .map(|(_, v)| v.clone())
        };
        assert_eq!(
            line(&stall, "Memory available (GB)").as_deref(),
            Some("2-4")
        );
        assert_eq!(line(&stall, "Swap free").as_deref(), Some("<10%"));
        assert_eq!(
            line(&stall, "Waiting for the disk (% of 10 s)").as_deref(),
            Some("60+")
        );
        assert_eq!(line(&stall, "Window thread").as_deref(), Some("disk"));
        assert_eq!(
            line(&stall, "Waiting in").as_deref(),
            Some("folio_wait_bit_common")
        );
        if cfg!(target_arch = "x86_64") {
            assert_eq!(line(&stall, "System call").as_deref(), Some("read"));
        }

        // a crash says the memory and the waits, not a thread's state
        let crash = with_machine(report(), &machine, false);
        assert_eq!(line(&crash, "Window thread"), None);

        // what the machine did not say is not said
        assert!(with_machine(report(), &MachineNow::default(), true)
            .details
            .is_empty());
    }

    #[test]
    fn this_machine_can_be_read() {
        let machine = now(None);
        if cfg!(target_os = "linux") {
            assert!(machine.memory_available.is_some(), "{machine:?}");
        }
    }
}
