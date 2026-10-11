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

/// What to present an AMD GPU to ROCm as (`HSA_OVERRIDE_GFX_VERSION`), for the architectures the
/// kernels of PyTorch's wheels are not built for but a sister architecture's run on: an RX 6000 card
/// (RDNA2, `gfx1031` and the others of its family) as the `gfx1030` the wheels have, and the RDNA3
/// cards and iGPUs under the `gfx1100` ones. Empty for an architecture that needs nothing, or that
/// nothing helps (the list is the first that has an answer, so a machine with an iGPU and a card is
/// given the card's). ComfyUI's own README gives the same two values.
pub fn rocm_gfx_override(targets: &[String]) -> &'static str {
    for target in targets {
        match target.trim() {
            "gfx1031" | "gfx1032" | "gfx1033" | "gfx1034" | "gfx1035" | "gfx1036" => {
                return "10.3.0"
            }
            "gfx1101" | "gfx1102" | "gfx1103" => return "11.0.0",
            "gfx90c" => return "9.0.0",
            _ => {}
        }
    }
    ""
}

/// What the context and the program need on top of the bytes of the weights.
pub const OVERHEAD_FACTOR: f64 = 1.2;
/// The part of the memory a model is comfortably given.
pub const RAM_FRACTION: f64 = 0.5;
/// The part of the memory beyond which a model does not fit at all.
pub const RAM_LIMIT: f64 = 0.8;

/// How a model of some size sits in a machine's memory.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fit {
    /// It fits with room to spare.
    Comfortable,
    /// It fits, and little else does.
    Tight,
    /// It is more than the machine can hold.
    TooBig,
}

/// What a model of `bytes` of weights needs of the memory, with the context and the program counted.
pub fn memory_needed(bytes: u64) -> u64 {
    (bytes as f64 * OVERHEAD_FACTOR) as u64
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

    #[test]
    fn an_rx_6000_card_is_presented_as_the_architecture_the_wheels_have() {
        let t = |names: &[&str]| names.iter().map(|n| n.to_string()).collect::<Vec<_>>();
        assert_eq!(rocm_gfx_override(&t(&["gfx1031"])), "10.3.0");
        assert_eq!(rocm_gfx_override(&t(&["gfx1030"])), "");
        assert_eq!(rocm_gfx_override(&t(&["gfx1102"])), "11.0.0");
        assert_eq!(rocm_gfx_override(&t(&["gfx1100"])), "");
        assert_eq!(rocm_gfx_override(&t(&["gfx90c"])), "9.0.0");
        assert_eq!(rocm_gfx_override(&t(&["gfx942"])), "");
        assert_eq!(rocm_gfx_override(&t(&[])), "");
        // an iGPU that needs an answer and a card that does not: the answer is the first's
        assert_eq!(rocm_gfx_override(&t(&["gfx1036", "gfx1100"])), "10.3.0");
    }
}
