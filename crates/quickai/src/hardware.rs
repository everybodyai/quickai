//! Rough, best-effort hardware sizing. All we actually detect is total
//! system RAM (via `sysinfo`) — no VRAM, no CPU generation — so both
//! things built on top of it are deliberately conservative estimates, not
//! real benchmarks:
//! - `recommend`: which already-known model is the sane default to
//!   pre-select on this machine.
//! - `speed_tier`: roughly how fast a given model+settings combination is
//!   likely to run, shown as a tortoise/ox/horse icon in the GUI.

use sysinfo::System;

use crate::models::ModelEntry;

const GB: u64 = 1024 * 1024 * 1024;

pub fn total_ram_bytes() -> u64 {
    System::new_all().total_memory()
}

pub(crate) fn format_bytes(n: u64) -> String {
    const UNITS: &[&str] = &["B", "KB", "MB", "GB", "TB"];
    let mut size = n as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit < UNITS.len() - 1 {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{n} B")
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

/// What the context and the program need on top of the bytes of the weights.
const OVERHEAD_FACTOR: f64 = 1.2;
/// The part of the memory a model is comfortably given.
const RAM_FRACTION: f64 = 0.5;
/// The part of the memory beyond which a model does not fit at all.
const RAM_LIMIT: f64 = 0.8;

/// How a model of some size sits in this machine's memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// It fits with room to spare.
    Comfortable,
    /// It fits, and little else does.
    Tight,
    /// It is more than the machine can hold.
    TooBig,
}

/// Whether `bytes` of weights fit in `total_ram_bytes`: comfortably in half of it with the overhead
/// counted, tightly in four fifths, not beyond. All that is known of the machine is its total memory
/// (no graphics memory), so this is a hint and not a promise; an unknown memory judges nothing.
pub fn fit(bytes: u64, total_ram_bytes: u64) -> Fit {
    if total_ram_bytes == 0 {
        return Fit::Comfortable;
    }
    let need = bytes as f64 * OVERHEAD_FACTOR;
    let ram = total_ram_bytes as f64;
    if need <= ram * RAM_FRACTION {
        Fit::Comfortable
    } else if need <= ram * RAM_LIMIT {
        Fit::Tight
    } else {
        Fit::TooBig
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum SpeedTier {
    Slow,
    Medium,
    Fast,
}

impl SpeedTier {
    pub fn emoji(&self) -> &'static str {
        match self {
            SpeedTier::Slow => "🐢",
            SpeedTier::Medium => "🐂",
            SpeedTier::Fast => "🐎",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            SpeedTier::Slow => "Slower",
            SpeedTier::Medium => "Medium",
            SpeedTier::Fast => "Fast",
        }
    }
}

/// Buckets `total_ram / model_size` into three tiers, then knocks the tier
/// down one notch if GPU offload is off, since CPU-only is meaningfully slower
/// than the same model with GPU layers on. `gpu_offload` is the engine's own
/// `gpu_offload` fact (for llama.cpp: GPU layers above 0). Falls back to
/// `Medium` when the model's size isn't known.
pub fn speed_tier(model_size_bytes: u64, total_ram_bytes: u64, gpu_offload: bool) -> SpeedTier {
    if model_size_bytes == 0 || total_ram_bytes == 0 {
        return SpeedTier::Medium;
    }
    let ratio = total_ram_bytes as f64 / model_size_bytes as f64;
    let tier = if ratio >= 8.0 {
        SpeedTier::Fast
    } else if ratio >= 3.0 {
        SpeedTier::Medium
    } else {
        SpeedTier::Slow
    };
    if !gpu_offload && tier == SpeedTier::Fast {
        SpeedTier::Medium
    } else {
        tier
    }
}

/// Picks, among models whose file size we know, the largest one that
/// comfortably fits this machine's RAM — leaving headroom for the OS and
/// for context/activation overhead on top of the raw file size — biasing
/// toward quality when the hardware allows it, and falling back to the
/// smallest available model on constrained machines.
///
/// "Fits in RAM" alone isn't a good proxy for "runs at a reasonable
/// speed" — a large model is slow to run token-by-token regardless of how
/// much spare RAM sits behind it, and that effect is far more pronounced
/// without GPU offload. So on top of the RAM check, this also caps how
/// large a model it will actively recommend: a modest cap when GPU offload
/// is off (so definitely CPU-bound), a looser one otherwise. We still have
/// no way to detect actual VRAM, so even the looser cap is conservative
/// rather than "trust the GPU with anything" — offload on only means it was
/// requested, not that there's enough VRAM for it to matter. If nothing fits both the RAM
/// budget and the speed cap, this falls back to the old RAM-only pick, and
/// finally to the smallest model available, rather than recommending
/// nothing.
pub fn recommend(models: &[ModelEntry], total_ram_bytes: u64, gpu_offload: bool) -> Option<String> {
    let mut sized: Vec<&ModelEntry> = models.iter().filter(|m| m.size_bytes > 0).collect();
    if sized.is_empty() {
        return None;
    }
    sized.sort_by_key(|m| m.size_bytes);

    let budget = total_ram_bytes as f64 * RAM_FRACTION;
    let fits_ram = |m: &&ModelEntry| (m.size_bytes as f64) * OVERHEAD_FACTOR <= budget;

    const CPU_ONLY_SPEED_CAP: u64 = 6 * GB;
    const GPU_REQUESTED_SPEED_CAP: u64 = 12 * GB;
    let speed_cap = if gpu_offload {
        GPU_REQUESTED_SPEED_CAP
    } else {
        CPU_ONLY_SPEED_CAP
    };

    sized
        .iter()
        .rev()
        .find(|m| fits_ram(m) && m.size_bytes <= speed_cap)
        .or_else(|| sized.iter().rev().find(|m| fits_ram(m)))
        .or(sized.first())
        .map(|m| m.filename.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn entry(filename: &str, size_bytes: u64) -> ModelEntry {
        ModelEntry {
            filename: filename.to_string(),
            path: PathBuf::from(filename),
            display: filename.to_string(),
            note: String::new(),
            bundled: false,
            size_bytes,
            companions: Vec::new(),
            mode: None,
        }
    }

    #[test]
    fn recommends_largest_model_that_fits_comfortably() {
        let models = vec![entry("small.gguf", GB), entry("big.gguf", 8 * GB)];
        // 32 GB RAM: budget is 16 GB, big model (8 GB * 1.2 = 9.6 GB) fits,
        // and 8 GB is under the GPU-requested speed cap (12 GB).
        assert_eq!(
            recommend(&models, 32 * GB, true),
            Some("big.gguf".to_string())
        );
    }

    #[test]
    fn falls_back_to_smallest_on_constrained_hardware() {
        let models = vec![entry("small.gguf", GB), entry("big.gguf", 8 * GB)];
        // 4 GB RAM: budget is 2 GB, neither fits the fits-check, so we fall
        // back to the smallest rather than recommending nothing.
        assert_eq!(
            recommend(&models, 4 * GB, true),
            Some("small.gguf".to_string())
        );
    }

    #[test]
    fn caps_size_more_tightly_when_gpu_offload_is_off() {
        let models = vec![entry("small.gguf", 4 * GB), entry("big.gguf", 8 * GB)];
        // 64 GB RAM: budget is 32 GB, so both models fit RAM comfortably.
        // With GPU offload requested, 8 GB is under the 12 GB cap, so the
        // bigger model wins on quality.
        assert_eq!(
            recommend(&models, 64 * GB, true),
            Some("big.gguf".to_string())
        );
        // With offload explicitly off (definitely CPU-bound), 8 GB exceeds
        // the tighter 6 GB cap, so the smaller/faster model wins instead —
        // same RAM, same models, different pick.
        assert_eq!(
            recommend(&models, 64 * GB, false),
            Some("small.gguf".to_string())
        );
    }

    #[test]
    fn falls_back_to_ram_only_pick_when_every_model_exceeds_the_speed_cap() {
        let models = vec![entry("huge.gguf", 20 * GB)];
        // 64 GB RAM: budget 32 GB, 20 GB * 1.2 = 24 GB fits, but 20 GB is
        // over even the looser GPU-requested speed cap. With nothing else
        // to recommend, fall back to the RAM-only pick rather than None.
        assert_eq!(
            recommend(&models, 64 * GB, true),
            Some("huge.gguf".to_string())
        );
    }

    #[test]
    fn speed_tier_downgrades_fast_to_medium_without_gpu_offload() {
        let size = GB;
        let ram = 32 * GB; // ratio 32 -> Fast with GPU offload
        assert_eq!(speed_tier(size, ram, true), SpeedTier::Fast);
        assert_eq!(speed_tier(size, ram, false), SpeedTier::Medium);
    }

    #[test]
    fn p3_j18_a_model_fits_comfortably_in_half_the_memory_tightly_in_four_fifths_and_not_beyond() {
        // 16 GB of memory: the weights plus a fifth are comfortable up to 8 GB of it, tight up to 12.8 GB
        let ram = 16 * GB;
        assert_eq!(fit(GB, ram), Fit::Comfortable);
        assert_eq!(
            fit(6 * GB, ram),
            Fit::Comfortable,
            "7.2 GB with the overhead"
        );
        assert_eq!(fit(7 * GB, ram), Fit::Tight, "8.4 GB with the overhead");
        assert_eq!(fit(10 * GB, ram), Fit::Tight, "12 GB with the overhead");
        assert_eq!(fit(11 * GB, ram), Fit::TooBig, "13.2 GB with the overhead");
        assert_eq!(fit(40 * GB, ram), Fit::TooBig);
        // the edges belong to the kinder side
        assert_eq!(fit((ram as f64 * 0.5 / 1.2) as u64, ram), Fit::Comfortable);
        assert_eq!(fit((ram as f64 * 0.8 / 1.2) as u64, ram), Fit::Tight);
        // a machine whose memory is not known is not judged, and an empty file always fits
        assert_eq!(fit(100 * GB, 0), Fit::Comfortable);
        assert_eq!(fit(0, ram), Fit::Comfortable);
        // the same model on a smaller machine
        assert_eq!(fit(3 * GB, 4 * GB), Fit::TooBig);
        assert_eq!(fit(3 * GB, 8 * GB), Fit::Comfortable, "3.6 GB of 8");
        assert_eq!(fit(4 * GB + GB / 2, 8 * GB), Fit::Tight, "5.4 GB of 8");
        assert_eq!(fit(3 * GB, 64 * GB), Fit::Comfortable);
    }

    #[test]
    fn speed_tier_unknown_size_is_medium() {
        assert_eq!(speed_tier(0, 32 * GB, true), SpeedTier::Medium);
    }
}
