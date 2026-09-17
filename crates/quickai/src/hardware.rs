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
/// down one notch if GPU offload is off (`n_gpu_layers == 0`), since
/// CPU-only is meaningfully slower than the same model with GPU layers on.
/// Falls back to `Medium` when the model's size isn't known.
pub fn speed_tier(model_size_bytes: u64, total_ram_bytes: u64, n_gpu_layers: u32) -> SpeedTier {
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
    if n_gpu_layers == 0 && tier == SpeedTier::Fast {
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
/// large a model it will actively recommend: a modest cap when
/// `n_gpu_layers` is 0 (offload explicitly off, so definitely CPU-bound),
/// a looser one otherwise. We still have no way to detect actual VRAM, so
/// even the looser cap is conservative rather than "trust the GPU with
/// anything" — `n_gpu_layers > 0` only means offload was requested, not
/// that there's enough VRAM for it to matter. If nothing fits both the RAM
/// budget and the speed cap, this falls back to the old RAM-only pick, and
/// finally to the smallest model available, rather than recommending
/// nothing.
pub fn recommend(models: &[ModelEntry], total_ram_bytes: u64, n_gpu_layers: u32) -> Option<String> {
    let mut sized: Vec<&ModelEntry> = models.iter().filter(|m| m.size_bytes > 0).collect();
    if sized.is_empty() {
        return None;
    }
    sized.sort_by_key(|m| m.size_bytes);

    const OVERHEAD_FACTOR: f64 = 1.2;
    const RAM_FRACTION: f64 = 0.5;
    let budget = total_ram_bytes as f64 * RAM_FRACTION;
    let fits_ram = |m: &&ModelEntry| (m.size_bytes as f64) * OVERHEAD_FACTOR <= budget;

    const CPU_ONLY_SPEED_CAP: u64 = 6 * GB;
    const GPU_REQUESTED_SPEED_CAP: u64 = 12 * GB;
    let speed_cap = if n_gpu_layers == 0 {
        CPU_ONLY_SPEED_CAP
    } else {
        GPU_REQUESTED_SPEED_CAP
    };

    sized
        .iter()
        .rev()
        .find(|m| fits_ram(m) && m.size_bytes <= speed_cap)
        .or_else(|| sized.iter().rev().find(|m| fits_ram(m)))
        .or(sized.first())
        .map(|m| m.filename.clone())
}

/// Suggests a context-size default for a model, based on its file size (a
/// rough proxy for how much KV-cache memory each context token costs — more
/// layers/wider hidden state in a bigger file) and how much RAM headroom is
/// left over once the model itself is loaded. Same caveats as `recommend`/
/// `speed_tier`: no VRAM detection, a rough heuristic rather than a real
/// computation from the model's actual architecture. The GUI's context-size
/// field stays a plain editable `DragValue` — this is only what it's reset
/// to when the selected model changes, not a hard cap.
pub fn recommend_ctx_size(model_size_bytes: u64, total_ram_bytes: u64) -> u32 {
    const GB: u64 = 1024 * 1024 * 1024;
    const DEFAULT: u32 = 4096;
    const MIN: u32 = 512;
    const MAX: u32 = 131_072; // matches the DragValue's own range

    if model_size_bytes == 0 || total_ram_bytes == 0 {
        return DEFAULT;
    }

    // Ceiling from the model's own footprint, before RAM is even considered.
    let ceiling: u32 = if model_size_bytes >= 15 * GB {
        4096
    } else if model_size_bytes >= 4 * GB {
        8192
    } else {
        16384
    };

    let ratio = total_ram_bytes as f64 / model_size_bytes as f64;
    let suggested = if ratio >= 8.0 {
        ceiling
    } else if ratio >= 3.0 {
        ceiling / 2
    } else {
        ceiling / 4
    };

    suggested.clamp(MIN, MAX)
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
        }
    }

    #[test]
    fn recommends_largest_model_that_fits_comfortably() {
        let models = vec![entry("small.gguf", GB), entry("big.gguf", 8 * GB)];
        // 32 GB RAM: budget is 16 GB, big model (8 GB * 1.2 = 9.6 GB) fits,
        // and 8 GB is under the GPU-requested speed cap (12 GB).
        assert_eq!(
            recommend(&models, 32 * GB, 999),
            Some("big.gguf".to_string())
        );
    }

    #[test]
    fn falls_back_to_smallest_on_constrained_hardware() {
        let models = vec![entry("small.gguf", GB), entry("big.gguf", 8 * GB)];
        // 4 GB RAM: budget is 2 GB, neither fits the fits-check, so we fall
        // back to the smallest rather than recommending nothing.
        assert_eq!(
            recommend(&models, 4 * GB, 999),
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
            recommend(&models, 64 * GB, 999),
            Some("big.gguf".to_string())
        );
        // With offload explicitly off (definitely CPU-bound), 8 GB exceeds
        // the tighter 6 GB cap, so the smaller/faster model wins instead —
        // same RAM, same models, different pick.
        assert_eq!(
            recommend(&models, 64 * GB, 0),
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
            recommend(&models, 64 * GB, 999),
            Some("huge.gguf".to_string())
        );
    }

    #[test]
    fn speed_tier_downgrades_fast_to_medium_without_gpu_offload() {
        let size = GB;
        let ram = 32 * GB; // ratio 32 -> Fast with GPU offload
        assert_eq!(speed_tier(size, ram, 999), SpeedTier::Fast);
        assert_eq!(speed_tier(size, ram, 0), SpeedTier::Medium);
    }

    #[test]
    fn speed_tier_unknown_size_is_medium() {
        assert_eq!(speed_tier(0, 32 * GB, 999), SpeedTier::Medium);
    }

    #[test]
    fn ctx_size_favors_larger_context_for_small_model_with_ample_ram() {
        // 1 GB model, 32 GB RAM: ratio 32 -> full ceiling for a small model.
        assert_eq!(recommend_ctx_size(GB, 32 * GB), 16384);
    }

    #[test]
    fn ctx_size_caps_lower_for_large_model_regardless_of_ram() {
        // 20 GB model, 160 GB RAM: ratio 8 -> full ceiling, but the ceiling
        // itself is capped lower because the model is large.
        assert_eq!(recommend_ctx_size(20 * GB, 160 * GB), 4096);
    }

    #[test]
    fn ctx_size_shrinks_on_constrained_ram() {
        // 8 GB model, 16 GB RAM: ratio 2 -> below the 3.0 threshold.
        let ceiling = 8192; // 4-15 GB model tier
        assert_eq!(recommend_ctx_size(8 * GB, 16 * GB), ceiling / 4);
    }

    #[test]
    fn ctx_size_unknown_size_or_ram_is_default() {
        assert_eq!(recommend_ctx_size(0, 32 * GB), 4096);
        assert_eq!(recommend_ctx_size(GB, 0), 4096);
    }
}
