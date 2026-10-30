//! The coarse measures telemetry sends instead of exact ones: memory in steps, durations in ranges, the hardware
//! class, the name a model may be sent under, an engine's exit, and the hour an event happened in.

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

/// The hardware class of telemetry, the community's filters and "computers like yours":
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

/// How long the app was open, in the ranges sent: `<5`, `5-30`, `30-60`, `60-180`, `180+` minutes.
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

/// How long the window did not answer, in the ranges sent.
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

/// The size of a model that is sent as `other`, in GB ranges.
pub const SIZES: [&str; 5] = ["<2", "2-5", "5-10", "10-20", "20+"];

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

/// What is known of a model, to name it in telemetry.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ModelFacts<'a> {
    /// Its id on the recommended list, when it is one of the list.
    pub catalog: Option<&'a str>,
    /// The public Hugging Face repository it was fetched from through a look-up (`owner/repo`).
    pub huggingface: Option<&'a str>,
    /// Its size in bytes, for the range an `other` model is sent with.
    pub size: u64,
}

/// The name a model is sent under: `catalog:<id>`, `hf:<owner>/<repo>`, or `other:<size range>`. A model that is
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

/// The hour (UTC) a moment falls in, as RFC 3339: events carry the hour they happened in, not the moment.
pub fn hour_utc(unix_seconds: u64) -> String {
    let days = unix_seconds / 86_400;
    let hour = (unix_seconds % 86_400) / 3_600;
    let (year, month, day) = civil_from_days(days as i64);
    format!("{year:04}-{month:02}-{day:02}T{hour:02}:00:00Z")
}

/// A moment (UTC) as RFC 3339, to the second: for the telemetry log on the person's own computer, never sent.
pub fn moment_utc(unix_seconds: u64) -> String {
    let (year, month, day) = civil_from_days((unix_seconds / 86_400) as i64);
    let secs = unix_seconds % 86_400;
    format!(
        "{year:04}-{month:02}-{day:02}T{:02}:{:02}:{:02}Z",
        secs / 3_600,
        (secs % 3_600) / 60,
        secs % 60
    )
}

/// The calendar date `days` after 1970-01-01 (Howard Hinnant's algorithm).
fn civil_from_days(days: i64) -> (i64, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = yoe + era * 400 + i64::from(month <= 2);
    (year, month, day)
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
        assert_eq!(size_bucket(1_100_000_000), "<2");
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
        assert_eq!(model_name(&odd), "other:<2");
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
    fn the_hour_is_utc_and_rfc_3339() {
        assert_eq!(hour_utc(0), "1970-01-01T00:00:00Z");
        // 2026-10-30T09:12:44Z
        assert_eq!(hour_utc(1_793_351_564), "2026-10-30T09:00:00Z");
        // a leap day
        assert_eq!(hour_utc(951_782_400), "2000-02-29T00:00:00Z");
        assert_eq!(moment_utc(1_793_351_564), "2026-10-30T09:12:44Z");
    }
}
