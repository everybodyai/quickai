//! Pure rules of thumb that documents may call by name (`recommend_ctx_size(...)`). They live in
//! the core so the planner, the form model and the headless tools all give the same answer as the
//! application. Their thresholds become configuration (`policy.heuristics`) in Phase 5; until then
//! they are the constants the application has always used.

const GB: u64 = 1024 * 1024 * 1024;

/// Suggests a context-size default for a model, based on its file size (a
/// rough proxy for how much KV-cache memory each context token costs — more
/// layers/wider hidden state in a bigger file) and how much RAM headroom is
/// left over once the model itself is loaded. Same caveats as the model
/// recommendation and speed tier: no VRAM detection, a rough heuristic rather
/// than a real computation from the model's actual architecture. The context-size
/// field stays an editable number — this is only what it is reset to when the
/// selected model changes, not a hard cap.
pub fn recommend_ctx_size(model_size_bytes: u64, total_ram_bytes: u64) -> u32 {
    const DEFAULT: u32 = 4096;
    const MIN: u32 = 512;
    const MAX: u32 = 131_072; // matches the field's own range

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

    #[test]
    fn favors_larger_context_for_small_model_with_ample_ram() {
        // 1 GB model, 32 GB RAM: ratio 32 -> full ceiling for a small model.
        assert_eq!(recommend_ctx_size(GB, 32 * GB), 16384);
    }

    #[test]
    fn caps_lower_for_large_model_regardless_of_ram() {
        // 20 GB model, 160 GB RAM: ratio 8 -> full ceiling, but the ceiling
        // itself is capped lower because the model is large.
        assert_eq!(recommend_ctx_size(20 * GB, 160 * GB), 4096);
    }

    #[test]
    fn shrinks_on_constrained_ram() {
        // 8 GB model, 16 GB RAM: ratio 2 -> below the 3.0 threshold.
        let ceiling = 8192; // 4-15 GB model tier
        assert_eq!(recommend_ctx_size(8 * GB, 16 * GB), ceiling / 4);
    }

    #[test]
    fn unknown_size_or_ram_is_the_default() {
        assert_eq!(recommend_ctx_size(0, 32 * GB), 4096);
        assert_eq!(recommend_ctx_size(GB, 0), 4096);
    }

    #[test]
    fn the_middle_ratio_halves_the_ceiling() {
        // 4 GB model, 16 GB RAM: ratio 4 -> half of the 8192 ceiling.
        assert_eq!(recommend_ctx_size(4 * GB, 16 * GB), 4096);
    }
}
