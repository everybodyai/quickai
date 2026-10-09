//! Best-effort GPU vendor sniff. The only thing this is used for is warning
//! when a llama.cpp build's backend is locked to a specific vendor (CUDA →
//! NVIDIA, ROCm/HIP → AMD) but that vendor's GPU doesn't seem to be present
//! on this machine — e.g. picking a CUDA build with an AMD card, which will
//! run (llama-server itself has no idea what GPU you meant) but silently
//! fall back to CPU. Vulkan and plain CPU builds are vendor-agnostic and
//! never trigger this.
//!
//! There's no portable, dependency-free way to query GPU hardware directly,
//! so this shells out to whatever OS-native tool lists display adapters and
//! pattern-matches vendor names out of the text. If that tool is missing,
//! fails, or its output doesn't match anything recognisable, this reports
//! no vendors detected rather than guessing — a warning that fails to show
//! up is much less annoying than a wrong one.

use std::process::Command;

use quickai_core::doc::engine::Backend;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GpuVendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
}

impl GpuVendor {
    pub fn label(&self) -> &'static str {
        match self {
            GpuVendor::Nvidia => "NVIDIA",
            GpuVendor::Amd => "AMD",
            GpuVendor::Intel => "Intel",
            GpuVendor::Apple => "Apple",
        }
    }

    /// The vendor an engine document names (`nvidia`, `amd`, `intel`, `apple`).
    pub fn from_key(key: &str) -> Option<GpuVendor> {
        match key {
            "nvidia" => Some(GpuVendor::Nvidia),
            "amd" => Some(GpuVendor::Amd),
            "intel" => Some(GpuVendor::Intel),
            "apple" => Some(GpuVendor::Apple),
            _ => None,
        }
    }
}

fn classify(line: &str) -> Option<GpuVendor> {
    let lower = line.to_ascii_lowercase();
    if lower.contains("nvidia") {
        Some(GpuVendor::Nvidia)
    } else if lower.contains("amd")
        || lower.contains("radeon")
        || lower.contains("advanced micro devices")
    {
        Some(GpuVendor::Amd)
    } else if lower.contains("intel") {
        Some(GpuVendor::Intel)
    } else if lower.contains("apple") {
        Some(GpuVendor::Apple)
    } else {
        None
    }
}

/// Lines of free text, each hopefully naming one display adapter — the raw
/// material `classify` turns into vendors. Split out from `detect` so tests
/// can exercise `classify` against canned output without needing the
/// underlying OS tool to be present.
#[cfg(target_os = "linux")]
fn adapter_lines() -> Vec<String> {
    let Ok(out) = Command::new("lspci").arg("-mm").output() else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "windows")]
fn adapter_lines() -> Vec<String> {
    let Ok(out) = Command::new("powershell")
        .args([
            "-NoProfile",
            "-Command",
            "Get-CimInstance Win32_VideoController | Select-Object -ExpandProperty Name",
        ])
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(target_os = "macos")]
fn adapter_lines() -> Vec<String> {
    let Ok(out) = Command::new("system_profiler")
        .arg("SPDisplaysDataType")
        .output()
    else {
        return Vec::new();
    };
    if !out.status.success() {
        return Vec::new();
    }
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::to_string)
        .collect()
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn adapter_lines() -> Vec<String> {
    Vec::new()
}

/// All distinct GPU vendors this machine seems to have (a laptop with an
/// integrated + discrete GPU can have more than one), in no particular
/// order. Empty means "couldn't tell" — callers should treat that as "don't
/// warn", not "no GPU".
pub fn detect() -> Vec<GpuVendor> {
    let mut vendors: Vec<GpuVendor> = adapter_lines().iter().filter_map(|l| classify(l)).collect();
    vendors.sort_by_key(|v| v.label());
    vendors.dedup();
    vendors
}

/// The name of the architecture a node of the kernel's compute topology (`/sys/class/kfd`) has:
/// its `gfx_target_version` is the number `MMmmss` (`100301`), and the name is the one ROCm uses
/// (`gfx1031`, with the minor number and the step in hexadecimal: `90010` is `gfx90a`). The CPU's
/// node has a version of 0 and no name.
pub fn target_from_properties(properties: &str) -> Option<String> {
    let version: u32 = properties.lines().find_map(|line| {
        let (key, value) = line.trim().split_once(char::is_whitespace)?;
        (key == "gfx_target_version").then(|| value.trim().parse().ok())?
    })?;
    if version == 0 {
        return None;
    }
    let (major, minor, step) = (version / 10_000, (version / 100) % 100, version % 100);
    Some(format!("gfx{major}{minor:x}{step:x}"))
}

/// The architectures of the AMD GPUs the kernel driver reports, as ROCm names them. Empty where the
/// driver does not say (it is not loaded, or the system is not Linux): that is not "no GPU".
#[cfg(target_os = "linux")]
pub fn amd_targets() -> Vec<String> {
    let Ok(nodes) = std::fs::read_dir("/sys/class/kfd/kfd/topology/nodes") else {
        return Vec::new();
    };
    let mut names: Vec<String> = nodes
        .flatten()
        .filter_map(|node| std::fs::read_to_string(node.path().join("properties")).ok())
        .filter_map(|text| target_from_properties(&text))
        .collect();
    names.sort();
    names.dedup();
    names
}

#[cfg(not(target_os = "linux"))]
pub fn amd_targets() -> Vec<String> {
    Vec::new()
}

/// If `backend` (a rule of the engine document's `backends`, see `acquire::backend`) needs a GPU
/// vendor that is not among `vendors`, a short warning explaining the mismatch, with `label` as the
/// backend's name. `None` if the backend is vendor-agnostic (Vulkan, SYCL, CPU) or `vendors` is
/// empty (nothing detected, so nothing to contradict).
pub fn mismatch_warning(vendors: &[GpuVendor], backend: &Backend, label: &str) -> Option<String> {
    let required = GpuVendor::from_key(backend.needs_gpu.as_deref()?)?;
    if vendors.is_empty() || vendors.contains(&required) {
        return None;
    }
    let found = vendors
        .iter()
        .map(|v| v.label())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "⚠ Targets {} GPUs via {label}, but this machine looks like it has {found} — \
         it likely won't use your GPU. A Vulkan build works across AMD/NVIDIA/Intel instead.",
        required.label()
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_recognises_known_vendors() {
        assert_eq!(classify("NVIDIA GeForce RTX 4070"), Some(GpuVendor::Nvidia));
        assert_eq!(classify("AMD Radeon RX 7800 XT"), Some(GpuVendor::Amd));
        assert_eq!(
            classify("Advanced Micro Devices, Inc. [AMD/ATI]"),
            Some(GpuVendor::Amd)
        );
        assert_eq!(
            classify("Intel(R) UHD Graphics 770"),
            Some(GpuVendor::Intel)
        );
        assert_eq!(classify("Apple M2"), Some(GpuVendor::Apple));
        assert_eq!(classify("Some Unknown Adapter"), None);
    }

    #[test]
    fn a_nodes_target_version_is_the_name_rocm_gives_it() {
        let node = |version: &str| {
            format!("cpu_cores_count 0\ngfx_target_version {version}\nvendor_id 4098\n")
        };
        assert_eq!(
            target_from_properties(&node("100301")).as_deref(),
            Some("gfx1031")
        );
        assert_eq!(
            target_from_properties(&node("110000")).as_deref(),
            Some("gfx1100")
        );
        assert_eq!(
            target_from_properties(&node("90010")).as_deref(),
            Some("gfx90a")
        );
        assert_eq!(
            target_from_properties(&node("90402")).as_deref(),
            Some("gfx942")
        );
        assert_eq!(
            target_from_properties(&node("120001")).as_deref(),
            Some("gfx1201")
        );
        // the CPU's node, and a file with no such line
        assert_eq!(target_from_properties(&node("0")), None);
        assert_eq!(target_from_properties("vendor_id 4098\n"), None);
        assert_eq!(target_from_properties(&node("none")), None);
    }

    fn backend(label: &str, needs_gpu: Option<&str>) -> Backend {
        Backend {
            needle: label.to_ascii_lowercase(),
            label: label.to_string(),
            needs_gpu: needs_gpu.map(str::to_string),
        }
    }

    #[test]
    fn mismatch_warning_flags_cuda_without_nvidia() {
        let warning = mismatch_warning(&[GpuVendor::Amd], &backend("CUDA", Some("nvidia")), "CUDA");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("NVIDIA"));
    }

    #[test]
    fn mismatch_warning_flags_hip_without_amd() {
        let warning = mismatch_warning(
            &[GpuVendor::Nvidia],
            &backend("ROCm/HIP", Some("amd")),
            "ROCm/HIP",
        );
        assert!(warning.is_some());
    }

    #[test]
    fn mismatch_warning_silent_when_vendor_matches() {
        let cuda = backend("CUDA", Some("nvidia"));
        let hip = backend("ROCm/HIP", Some("amd"));
        assert_eq!(mismatch_warning(&[GpuVendor::Nvidia], &cuda, "CUDA"), None);
        assert_eq!(mismatch_warning(&[GpuVendor::Amd], &hip, "ROCm/HIP"), None);
        assert_eq!(
            mismatch_warning(&[GpuVendor::Amd, GpuVendor::Nvidia], &cuda, "CUDA"),
            None,
            "any detected card of the vendor is enough"
        );
    }

    #[test]
    fn mismatch_warning_silent_for_vendor_agnostic_backends() {
        for label in ["Vulkan", "CPU", "SYCL"] {
            assert_eq!(
                mismatch_warning(&[GpuVendor::Amd], &backend(label, None), label),
                None,
                "{label}"
            );
        }
    }

    #[test]
    fn mismatch_warning_silent_when_nothing_detected() {
        assert_eq!(
            mismatch_warning(&[], &backend("CUDA", Some("nvidia")), "CUDA"),
            None
        );
    }

    #[test]
    fn mismatch_warning_names_both_sides() {
        let warning = mismatch_warning(
            &[GpuVendor::Amd, GpuVendor::Intel],
            &backend("CUDA", Some("nvidia")),
            "CUDA",
        )
        .unwrap();
        assert!(warning.starts_with(
            "⚠ Targets NVIDIA GPUs via CUDA, but this machine looks like it has AMD, Intel"
        ));
        assert!(warning.ends_with("A Vulkan build works across AMD/NVIDIA/Intel instead."));
    }

    #[test]
    fn a_vendor_the_document_names_wrongly_flags_nothing() {
        assert_eq!(GpuVendor::from_key("nvidlia"), None);
        assert_eq!(
            mismatch_warning(&[GpuVendor::Amd], &backend("X", Some("nvidlia")), "X"),
            None
        );
        for (key, vendor) in [
            ("nvidia", GpuVendor::Nvidia),
            ("amd", GpuVendor::Amd),
            ("intel", GpuVendor::Intel),
            ("apple", GpuVendor::Apple),
        ] {
            assert_eq!(GpuVendor::from_key(key), Some(vendor));
            assert_eq!(
                vendor.label().to_ascii_lowercase(),
                key,
                "the key is the lower-case label"
            );
        }
    }

    /// The old functions decided by the file name's words; the embedded document must decide the same
    /// way for the names llama.cpp publishes.
    #[test]
    fn the_embedded_documents_backends_decide_as_the_old_code_did() {
        let registry = crate::config::build_registry(&crate::config::ConfigDirs::default());
        let engine = registry.engine("llamacpp").unwrap();
        // (file name, label, warns with only an AMD card, warns with only an NVIDIA card)
        for (name, label, amd_warns, nvidia_warns) in [
            (
                "llama-b11007-bin-win-cuda-cu12.4-x64.zip",
                "CUDA",
                true,
                false,
            ),
            (
                "llama-b11007-bin-win-vulkan-x64.zip",
                "Vulkan",
                false,
                false,
            ),
            ("llama-b11007-bin-win-sycl-x64.zip", "SYCL", false, false),
            ("llama-b11007-bin-win-hip-x64.zip", "ROCm/HIP", false, true),
            (
                "llama-b11007-bin-ubuntu-rocm-7.0-x64.tar.gz",
                "ROCm/HIP",
                false,
                true,
            ),
            ("llama-b11007-bin-win-cpu-x64.zip", "CPU", false, false),
            ("llama-b11007-bin-macos-arm64.zip", "default", false, false),
        ] {
            assert_eq!(
                quickai_core::acquire::backend_label(engine, name),
                label,
                "{name}"
            );
            let backend = quickai_core::acquire::backend(engine, name);
            let warns = |vendors: &[GpuVendor]| {
                backend.is_some_and(|b| mismatch_warning(vendors, b, label).is_some())
            };
            assert_eq!(
                warns(&[GpuVendor::Amd]),
                amd_warns,
                "{name} with an AMD card"
            );
            assert_eq!(
                warns(&[GpuVendor::Nvidia]),
                nvidia_warns,
                "{name} with an NVIDIA card"
            );
            assert!(!warns(&[]), "{name} with nothing detected");
        }
    }
}
