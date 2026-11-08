//! The coarse measures a problem report says instead of exact ones: memory in steps, durations in ranges, the name a
//! model may be given, and an engine's exit. A range says enough to fix a bug and nothing that tells one computer
//! from another.

/// The memory buckets, in GB, and the most memory (GiB) each one takes: a computer is put in the first bucket whose
/// limit it does not pass, so 15.6 GiB of usable memory is a 16 GB computer.
const MEMORY: [(u32, u64); 4] = [(8, 12), (16, 24), (32, 48), (64, 96)];

/// The memory bucket of a computer with `bytes` of memory: 8, 16, 32, 64 or 128 (for 128 GB and more).
pub fn memory_bucket_gb(bytes: u64) -> u32 {
    let gib = bytes / (1024 * 1024 * 1024);
    MEMORY
        .iter()
        .find(|(_, most)| gib <= *most)
        .map(|(bucket, _)| *bucket)
        .unwrap_or(128)
}

/// The hardware class of the community's filters and "computers like yours":
/// `<os>-<arch>-<gpu makers, sorted, or "cpu">-<memory bucket>gb`, e.g. `linux-x86_64-amd-32gb`.
pub fn hardware_class(os: &str, arch: &str, gpus: &[String], memory_gb: u32) -> String {
    let mut makers: Vec<&str> = gpus.iter().map(String::as_str).collect();
    makers.sort_unstable();
    makers.dedup();
    let gpu = if makers.is_empty() {
        "cpu".to_string()
    } else {
        makers.join("+")
    };
    format!("{os}-{arch}-{gpu}-{memory_gb}gb")
}

/// How long the app was open, in the ranges a report says: `<5`, `5-30`, `30-60`, `60-180`, `180+` minutes.
pub const MINUTES: [&str; 5] = ["<5", "5-30", "30-60", "60-180", "180+"];

/// The range of `MINUTES` that `minutes` falls in.
pub fn minutes_bucket(minutes: u64) -> &'static str {
    match minutes {
        0..5 => MINUTES[0],
        5..30 => MINUTES[1],
        30..60 => MINUTES[2],
        60..180 => MINUTES[3],
        _ => MINUTES[4],
    }
}

/// How long the window did not answer, in the ranges a report says.
pub const STALL: [&str; 4] = ["5-10", "10-30", "30-60", "60+"];

/// The range of `STALL` that `seconds` falls in (a stall is five seconds or more).
pub fn stall_bucket(seconds: u64) -> &'static str {
    match seconds {
        0..10 => STALL[0],
        10..30 => STALL[1],
        30..60 => STALL[2],
        _ => STALL[3],
    }
}

/// The size of a model that is named `other`, in GB ranges. Plain words: they are part of a name (`other:0-2`).
pub const SIZES: [&str; 5] = ["0-2", "2-5", "5-10", "10-20", "20+"];

/// The range of `SIZES` that a file of `bytes` falls in.
pub fn size_bucket(bytes: u64) -> &'static str {
    let gb = bytes / 1_000_000_000;
    match gb {
        0..2 => SIZES[0],
        2..5 => SIZES[1],
        5..10 => SIZES[2],
        10..20 => SIZES[3],
        _ => SIZES[4],
    }
}

/// What the window was showing that runs on without it when a stall or a crash happened: nothing, a download, an
/// install, a delete or a look-up (of the engine or of the program that goes with it).
pub const RUNNING: [&str; 5] = ["none", "download", "install", "delete", "look-up"];

/// The state of a thread, as the system says it: running, sleeping, waiting on the disk or a device (`disk`, which
/// nothing can interrupt), stopped, ended, idle, or another.
pub const THREAD_STATES: [&str; 7] = [
    "running", "sleeping", "disk", "stopped", "ended", "idle", "other",
];

/// The word of `THREAD_STATES` for a state letter of `/proc/<pid>/task/<tid>/stat`.
pub fn thread_state(letter: &str) -> &'static str {
    match letter {
        "R" => THREAD_STATES[0],
        "S" => THREAD_STATES[1],
        "D" => THREAD_STATES[2],
        "T" | "t" => THREAD_STATES[3],
        "Z" | "X" => THREAD_STATES[4],
        "I" => THREAD_STATES[5],
        _ => THREAD_STATES[6],
    }
}

/// The memory the system could still give, in GB ranges.
pub const AVAILABLE: [&str; 6] = ["<1", "1-2", "2-4", "4-8", "8-16", "16+"];

/// The range of `AVAILABLE` that `bytes` falls in.
pub fn available_bucket(bytes: u64) -> &'static str {
    let mib = bytes / (1024 * 1024);
    match mib {
        0..1024 => AVAILABLE[0],
        1024..2048 => AVAILABLE[1],
        2048..4096 => AVAILABLE[2],
        4096..8192 => AVAILABLE[3],
        8192..16384 => AVAILABLE[4],
        _ => AVAILABLE[5],
    }
}

/// How much of the swap was free: none set up, or a share of it.
pub const SWAP_FREE: [&str; 4] = ["no-swap", "<10%", "10-50%", "50%+"];

/// The range of `SWAP_FREE` for `free` bytes of `total`.
pub fn swap_free_bucket(free: u64, total: u64) -> &'static str {
    if total == 0 {
        return SWAP_FREE[0];
    }
    match free.saturating_mul(100) / total {
        0..10 => SWAP_FREE[1],
        10..50 => SWAP_FREE[2],
        _ => SWAP_FREE[3],
    }
}

/// The share of the last ten seconds in which something on the computer waited (for memory, or for the disk), in
/// percent ranges, as the system's pressure files say it.
pub const PRESSURE: [&str; 5] = ["<1", "1-10", "10-30", "30-60", "60+"];

/// The range of `PRESSURE` that `percent` falls in.
pub fn pressure_bucket(percent: f64) -> &'static str {
    match percent {
        p if p < 1.0 => PRESSURE[0],
        p if p < 10.0 => PRESSURE[1],
        p if p < 30.0 => PRESSURE[2],
        p if p < 60.0 => PRESSURE[3],
        _ => PRESSURE[4],
    }
}

/// The memory QuickAI itself holds, in ranges.
pub const APP_MEMORY: [&str; 4] = ["<256MB", "256MB-1GB", "1-4GB", "4GB+"];

/// The range of `APP_MEMORY` that `bytes` falls in.
pub fn app_memory_bucket(bytes: u64) -> &'static str {
    let mib = bytes / (1024 * 1024);
    match mib {
        0..256 => APP_MEMORY[0],
        256..1024 => APP_MEMORY[1],
        1024..4096 => APP_MEMORY[2],
        _ => APP_MEMORY[3],
    }
}

/// How far a download got before it failed.
pub const PROGRESS: [&str; 6] = ["0%", "1-25%", "25-50%", "50-75%", "75-99%", "100%"];

/// The range of `PROGRESS` for `done` bytes of `total`.
pub fn progress_bucket(done: u64, total: u64) -> &'static str {
    if done == 0 || total == 0 {
        return PROGRESS[0];
    }
    if done >= total {
        return PROGRESS[5];
    }
    match done.saturating_mul(100) / total {
        0..25 => PROGRESS[1],
        25..50 => PROGRESS[2],
        50..75 => PROGRESS[3],
        _ => PROGRESS[4],
    }
}

/// A function of the system's kernel as a report may say it (`do_epoll_wait`): its letters, digits, `_` and `.`, at most
/// 64. `None` when there is none (`0`, empty) or it has anything else in it.
pub fn kernel_function(name: &str) -> Option<String> {
    let name = name.trim();
    (!name.is_empty()
        && name != "0"
        && name.len() <= 64
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '.'))
    .then(|| name.to_string())
}

/// What is known of a model, to name it in a report.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelFacts<'a> {
    /// Its id on the recommended list, when it is one of the list.
    pub catalog: Option<&'a str>,
    /// The public Hugging Face repository it was fetched from through a look-up (`owner/repo`).
    pub huggingface: Option<&'a str>,
    /// Its size in bytes, for the range an `other` model is given.
    pub size: u64,
}

/// The name a report gives a model: `catalog:<id>`, `hf:<owner>/<repo>`, or `other:<size range>`. A model that is
/// neither on the list nor from a public repository is never named: its file name may be the person's own.
pub fn model_name(facts: &ModelFacts) -> String {
    if let Some(id) = facts.catalog.filter(|id| is_plain_name(id)) {
        return format!("catalog:{id}");
    }
    if let Some(repo) = facts
        .huggingface
        .filter(|r| r.split('/').count() == 2 && is_plain_name(r))
    {
        return format!("hf:{repo}");
    }
    format!("other:{}", size_bucket(facts.size))
}

/// A name made of letters, digits and `._:/+@-` only, as ids, labels and repositories are.
pub fn is_plain_name(text: &str) -> bool {
    !text.is_empty()
        && text.len() <= 160
        && text
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "._:/+@-".contains(c))
}

/// How an engine ended, from the line the runtime logs (`Server process exited unexpectedly (exit status: 1).`, or
/// `signal: 9 (SIGKILL)` on Unix, `exit code: 3221225477` on Windows): `exit 1`, `signal 9`, or `None` when the line
/// says neither.
pub fn exit_status(line: &str) -> Option<String> {
    let number_after = |key: &str| -> Option<String> {
        let rest = &line[line.find(key)? + key.len()..];
        let digits: String = rest
            .trim_start()
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '-')
            .collect();
        (!digits.is_empty()).then_some(digits)
    };
    if let Some(n) = number_after("signal:") {
        return Some(format!("signal {n}"));
    }
    number_after("exit status:")
        .or_else(|| number_after("exit code:"))
        .map(|n| format!("exit {n}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    const GIB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn memory_falls_in_its_step() {
        assert_eq!(memory_bucket_gb(4 * GIB), 8);
        assert_eq!(memory_bucket_gb(15 * GIB + GIB / 2), 16);
        assert_eq!(memory_bucket_gb(31 * GIB), 32);
        assert_eq!(memory_bucket_gb(64 * GIB), 64);
        assert_eq!(memory_bucket_gb(512 * GIB), 128);
    }

    #[test]
    fn the_hardware_class_sorts_the_makers_and_names_a_computer_without_any() {
        let gpus = vec!["nvidia".to_string(), "intel".to_string()];
        assert_eq!(
            hardware_class("linux", "x86_64", &gpus, 32),
            "linux-x86_64-intel+nvidia-32gb"
        );
        assert_eq!(
            hardware_class("windows", "x86_64", &[], 16),
            "windows-x86_64-cpu-16gb"
        );
    }

    #[test]
    fn durations_and_sizes_fall_in_ranges() {
        assert_eq!(minutes_bucket(0), "<5");
        assert_eq!(minutes_bucket(45), "30-60");
        assert_eq!(minutes_bucket(600), "180+");
        assert_eq!(stall_bucket(5), "5-10");
        assert_eq!(stall_bucket(75), "60+");
        assert_eq!(size_bucket(1_100_000_000), "0-2");
        assert_eq!(size_bucket(6_140_000_000), "5-10");
    }

    #[test]
    fn a_model_is_named_only_when_it_is_public() {
        let listed = ModelFacts {
            catalog: Some("qwen3-8b-q4_k_m"),
            huggingface: Some("Qwen/Qwen3-8B-GGUF"),
            size: 5_000_000_000,
        };
        assert_eq!(model_name(&listed), "catalog:qwen3-8b-q4_k_m");
        let looked_up = ModelFacts {
            huggingface: Some("Qwen/Qwen3-8B-GGUF"),
            size: 5_000_000_000,
            ..ModelFacts::default()
        };
        assert_eq!(model_name(&looked_up), "hf:Qwen/Qwen3-8B-GGUF");
        let own = ModelFacts {
            size: 2_500_000_000,
            ..ModelFacts::default()
        };
        assert_eq!(model_name(&own), "other:2-5");
        // a "repository" that is a path, or has spaces, is not one
        let odd = ModelFacts {
            huggingface: Some("my secret/model"),
            size: 1,
            ..ModelFacts::default()
        };
        assert_eq!(model_name(&odd), "other:0-2");
    }

    #[test]
    fn every_name_a_model_can_be_given_is_a_plain_name() {
        for size in [
            0,
            2_500_000_000,
            6_000_000_000,
            15_000_000_000,
            50_000_000_000,
        ] {
            let name = model_name(&ModelFacts {
                size,
                ..ModelFacts::default()
            });
            assert!(is_plain_name(&name), "{name}");
        }
    }

    #[test]
    fn an_exit_is_read_from_the_runtimes_line() {
        assert_eq!(
            exit_status("Server process exited unexpectedly (exit status: 1)."),
            Some("exit 1".to_string())
        );
        assert_eq!(
            exit_status("Server process exited unexpectedly (exit status: signal: 9 (SIGKILL))."),
            Some("signal 9".to_string())
        );
        assert_eq!(
            exit_status("Server process exited unexpectedly (exit status: exit code: 3221225477)."),
            Some("exit 3221225477".to_string())
        );
        assert_eq!(exit_status("Failed to launch server process"), None);
    }

    #[test]
    fn the_moment_of_a_stall_or_a_crash_is_said_in_ranges() {
        const GIB: u64 = 1024 * 1024 * 1024;
        assert_eq!(available_bucket(512 * 1024 * 1024), "<1");
        assert_eq!(available_bucket(3 * GIB), "2-4");
        assert_eq!(available_bucket(40 * GIB), "16+");
        assert_eq!(swap_free_bucket(0, 0), "no-swap");
        assert_eq!(swap_free_bucket(1, 100), "<10%");
        assert_eq!(swap_free_bucket(30, 100), "10-50%");
        assert_eq!(swap_free_bucket(100, 100), "50%+");
        assert_eq!(pressure_bucket(0.0), "<1");
        assert_eq!(pressure_bucket(61.25), "60+");
        assert_eq!(app_memory_bucket(300 * 1024 * 1024), "256MB-1GB");
        assert_eq!(progress_bucket(0, 100), "0%");
        assert_eq!(
            progress_bucket(10, 0),
            "0%",
            "no total: nothing can be said"
        );
        assert_eq!(progress_bucket(60, 100), "50-75%");
        assert_eq!(progress_bucket(99, 100), "75-99%");
        assert_eq!(progress_bucket(100, 100), "100%");
        assert_eq!(thread_state("D"), "disk");
        assert_eq!(thread_state("?"), "other");
        assert_eq!(
            kernel_function("do_epoll_wait"),
            Some("do_epoll_wait".into())
        );
        assert_eq!(kernel_function("0"), None);
        assert_eq!(kernel_function("/home/grace"), None);
    }
}
