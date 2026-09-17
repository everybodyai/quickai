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

/// If `backend_label` (see `llama_releases::backend_label`) names a vendor-
/// locked backend that doesn't match any of `vendors`, a short warning
/// explaining the mismatch — `None` if the backend is vendor-agnostic
/// (Vulkan/SYCL/CPU/unknown) or `vendors` is empty (nothing detected, so
/// nothing to contradict).
pub fn mismatch_warning(vendors: &[GpuVendor], backend_label: &str) -> Option<String> {
    let required = match backend_label {
        "CUDA" => GpuVendor::Nvidia,
        "ROCm/HIP" => GpuVendor::Amd,
        _ => return None,
    };
    if vendors.is_empty() || vendors.contains(&required) {
        return None;
    }
    let found = vendors
        .iter()
        .map(|v| v.label())
        .collect::<Vec<_>>()
        .join(", ");
    Some(format!(
        "⚠ Targets {} GPUs via {backend_label}, but this machine looks like it has {found} — \
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
    fn mismatch_warning_flags_cuda_without_nvidia() {
        let warning = mismatch_warning(&[GpuVendor::Amd], "CUDA");
        assert!(warning.is_some());
        assert!(warning.unwrap().contains("NVIDIA"));
    }

    #[test]
    fn mismatch_warning_flags_hip_without_amd() {
        let warning = mismatch_warning(&[GpuVendor::Nvidia], "ROCm/HIP");
        assert!(warning.is_some());
    }

    #[test]
    fn mismatch_warning_silent_when_vendor_matches() {
        assert_eq!(mismatch_warning(&[GpuVendor::Nvidia], "CUDA"), None);
        assert_eq!(mismatch_warning(&[GpuVendor::Amd], "ROCm/HIP"), None);
    }

    #[test]
    fn mismatch_warning_silent_for_vendor_agnostic_backends() {
        assert_eq!(mismatch_warning(&[GpuVendor::Amd], "Vulkan"), None);
        assert_eq!(mismatch_warning(&[GpuVendor::Amd], "CPU"), None);
        assert_eq!(mismatch_warning(&[GpuVendor::Amd], "default"), None);
    }

    #[test]
    fn mismatch_warning_silent_when_nothing_detected() {
        assert_eq!(mismatch_warning(&[], "CUDA"), None);
    }
}
