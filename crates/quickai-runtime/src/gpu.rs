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
fn adapter_lines() -> Option<Vec<String>> {
    let out = Command::new("lspci").arg("-mm").output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter(|l| l.contains("VGA compatible controller") || l.contains("3D controller"))
            .map(str::to_string)
            .collect(),
    )
}

#[cfg(target_os = "windows")]
fn adapter_lines() -> Option<Vec<String>> {
    Some(windows_adapter_lines())
}

#[cfg(target_os = "windows")]
fn windows_adapter_lines() -> Vec<String> {
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
fn adapter_lines() -> Option<Vec<String>> {
    Some(macos_adapter_lines())
}

#[cfg(target_os = "macos")]
fn macos_adapter_lines() -> Vec<String> {
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
fn adapter_lines() -> Option<Vec<String>> {
    Some(Vec::new())
}

/// The vendors a machine has, from what the tool said and from the files the kernel keeps, below `root`
/// (the real one is `/`; a test makes its own tree). A minimal server image has no `lspci`, and a container
/// that is given a card has the driver's files and not the bus: neither is a reason to say the machine has
/// no card, which is what a missing vendor is taken to mean (PyTorch for the processor).
///
/// A device of the display class (`0x03....`: a VGA controller, a 3D controller, another display
/// controller) is a card whose vendor number says whose; the NVIDIA driver's list of cards and its device
/// nodes say NVIDIA's. The result is sorted by name and has each vendor once, as `detect` has always given
/// it. Empty means "could not tell".
pub fn vendors_in(root: &std::path::Path, from_tool: Vec<GpuVendor>) -> Vec<GpuVendor> {
    let mut vendors = from_tool;
    if let Ok(devices) = std::fs::read_dir(root.join("sys/bus/pci/devices")) {
        for device in devices.flatten() {
            let number = |file: &str| {
                let text = std::fs::read_to_string(device.path().join(file)).ok()?;
                u32::from_str_radix(text.trim().trim_start_matches("0x"), 16).ok()
            };
            let (Some(class), Some(vendor)) = (number("class"), number("vendor")) else {
                continue;
            };
            if class >> 16 != 0x03 {
                continue;
            }
            match vendor {
                0x10de => vendors.push(GpuVendor::Nvidia),
                0x1002 => vendors.push(GpuVendor::Amd),
                0x8086 => vendors.push(GpuVendor::Intel),
                _ => {}
            }
        }
    }
    let has_card =
        |dir: &str| std::fs::read_dir(root.join(dir)).is_ok_and(|mut cards| cards.next().is_some());
    if has_card("proc/driver/nvidia/gpus") || root.join("dev/nvidia0").exists() {
        vendors.push(GpuVendor::Nvidia);
    }
    vendors.sort_by_key(|v| v.label());
    vendors.dedup();
    vendors
}

/// The vendors a person named (`nvidia`, `amd,intel`), or none (`none`): for a machine whose cards cannot be
/// told, or told wrongly.
pub fn vendors_named(words: &str) -> Result<Vec<GpuVendor>, String> {
    let mut vendors = Vec::new();
    for word in words.split(',').map(|w| w.trim().to_ascii_lowercase()) {
        match word.as_str() {
            "none" => {}
            "" => {}
            other => vendors.push(GpuVendor::from_key(other).ok_or_else(|| {
                format!("`{other}` is not a kind of card: say nvidia, amd, intel, apple or none")
            })?),
        }
    }
    vendors.sort_by_key(|v| v.label());
    vendors.dedup();
    Ok(vendors)
}

/// All distinct GPU vendors this machine seems to have (a laptop with an
/// integrated + discrete GPU can have more than one), in no particular
/// order. Empty means "couldn't tell" — callers should treat that as "don't
/// warn", not "no GPU".
pub fn detect() -> Vec<GpuVendor> {
    detect_from(adapter_lines(), std::path::Path::new("/"))
}

/// `detect` for what the tool answered (`None`: it could not be run) and the root of the files the kernel
/// keeps. A tool that answered is believed, even when it says nothing: a computer it lists no card on has
/// none, and whoever puts a stand-in of their own first on the path (the GUI suite does) decides what the
/// application is told. The kernel's own files are for a computer that has no such tool (a server's minimal
/// image) and not a second opinion.
fn detect_from(tool: Option<Vec<String>>, root: &std::path::Path) -> Vec<GpuVendor> {
    let from_tool: Vec<GpuVendor> = tool.iter().flatten().filter_map(|l| classify(l)).collect();
    if tool.is_none() && cfg!(target_os = "linux") {
        return vendors_in(root, from_tool);
    }
    let mut vendors = from_tool;
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

    // the kernel's names of devices have colons, which a file name on Windows cannot
    /// A tree the way the kernel lays out what a machine has: `sys/bus/pci/devices/<address>/{class,vendor}`.
    #[cfg(unix)]
    fn pci(root: &std::path::Path, address: &str, class: &str, vendor: &str) {
        let dir = root.join("sys/bus/pci/devices").join(address);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("class"), format!("{class}\n")).unwrap();
        std::fs::write(dir.join("vendor"), format!("{vendor}\n")).unwrap();
    }

    #[cfg(unix)]
    fn scratch(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("quickai-gpu-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// O40: a rented server often has no `lspci` (pciutils is not in a minimal image), and a machine whose
    /// card is not found is given PyTorch for the processor: the kernel's own list says what is there.
    #[cfg(unix)]
    #[test]
    fn p3_o40_a_card_is_found_in_the_kernels_list_of_devices_when_lspci_is_not_there() {
        let root = scratch("o40");
        pci(&root, "0000:00:02.0", "0x030000", "0x8086"); // a display controller of Intel's
        pci(&root, "0000:01:00.0", "0x030200", "0x10de"); // a 3D controller of NVIDIA's (a compute card)
        pci(&root, "0000:02:00.0", "0x038000", "0x1002"); // another display controller, AMD's
        pci(&root, "0000:00:1f.3", "0x040300", "0x8086"); // audio: not a card
        pci(&root, "0000:03:00.0", "0x020000", "0x10de"); // a network card of the same maker: not a card for this
        assert_eq!(
            vendors_in(&root, Vec::new()),
            [GpuVendor::Amd, GpuVendor::Intel, GpuVendor::Nvidia],
            "the three display devices, and none of the others, sorted by name"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// O41: a container that is given a card has the driver's files and no list of the bus; the driver
    /// says it is NVIDIA's.
    #[cfg(unix)]
    #[test]
    fn p3_o41_the_drivers_own_files_say_a_card_is_nvidias_when_the_bus_is_not_shown() {
        let by_proc = scratch("o41a");
        std::fs::create_dir_all(by_proc.join("proc/driver/nvidia/gpus/0000:01:00.0")).unwrap();
        assert_eq!(vendors_in(&by_proc, Vec::new()), [GpuVendor::Nvidia]);
        let by_node = scratch("o41b");
        std::fs::create_dir_all(by_node.join("dev")).unwrap();
        std::fs::write(by_node.join("dev/nvidia0"), "").unwrap();
        assert_eq!(vendors_in(&by_node, Vec::new()), [GpuVendor::Nvidia]);
        let empty_dir = scratch("o41c");
        std::fs::create_dir_all(empty_dir.join("proc/driver/nvidia/gpus")).unwrap();
        assert_eq!(
            vendors_in(&empty_dir, Vec::new()),
            Vec::<GpuVendor>::new(),
            "a driver with no card in its list is no card"
        );
        for dir in [by_proc, by_node, empty_dir] {
            let _ = std::fs::remove_dir_all(dir);
        }
    }

    /// O42: what the tool and the kernel both say is said once, and a machine the kernel says nothing of is
    /// not said to have no card: nothing found stays "could not tell".
    #[cfg(unix)]
    #[test]
    fn p3_o42_what_both_say_is_said_once_and_nothing_found_is_nothing() {
        let root = scratch("o42");
        pci(&root, "0000:01:00.0", "0x030000", "0x10de");
        assert_eq!(
            vendors_in(&root, vec![GpuVendor::Nvidia, GpuVendor::Intel]),
            [GpuVendor::Intel, GpuVendor::Nvidia]
        );
        let nothing = scratch("o42b");
        assert!(vendors_in(&nothing, Vec::new()).is_empty());
        // a file that is not a number is skipped, not guessed at
        pci(&nothing, "0000:09:00.0", "not-a-class", "0x10de");
        pci(&nothing, "0000:0a:00.0", "0x030000", "banana");
        assert!(vendors_in(&nothing, Vec::new()).is_empty());
        let _ = std::fs::remove_dir_all(&root);
        let _ = std::fs::remove_dir_all(&nothing);
    }

    /// O44: a tool that answered is believed, even when it says nothing; the kernel's files are for a computer that
    /// has no such tool. (The GUI suite puts a stand-in `lspci` first on the path to decide what the application is
    /// told; a fallback that looked past it would make the suite depend on the card of the computer it runs on.)
    #[cfg(target_os = "linux")]
    #[test]
    fn p3_o44_a_tool_that_answered_is_believed_and_the_kernels_files_are_for_one_that_is_missing() {
        let root = scratch("o44");
        pci(&root, "0000:01:00.0", "0x030000", "0x1002"); // the computer has a card of AMD's
        assert_eq!(
            detect_from(Some(Vec::new()), &root),
            Vec::<GpuVendor>::new(),
            "lspci ran and listed no card: that is the answer"
        );
        assert_eq!(
            detect_from(
                Some(vec![
                    "Slot:\t01:00.0\nClass:\tVGA compatible controller NVIDIA".to_string()
                ]),
                &root
            ),
            [GpuVendor::Nvidia],
            "and what it lists is what there is, whatever the kernel's files say"
        );
        assert_eq!(
            detect_from(None, &root),
            [GpuVendor::Amd],
            "no lspci at all: the kernel's list of devices"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    /// O43: a person can say what the machine has when it cannot be told (`quickai-run --gpu`), and the
    /// words they use are the vendors' own keys.
    #[test]
    fn p3_o43_a_person_can_name_the_cards_and_none_is_a_word_too() {
        assert_eq!(vendors_named("nvidia"), Ok(vec![GpuVendor::Nvidia]));
        assert_eq!(
            vendors_named("amd,intel"),
            Ok(vec![GpuVendor::Amd, GpuVendor::Intel])
        );
        assert_eq!(vendors_named("NVIDIA, nvidia"), Ok(vec![GpuVendor::Nvidia]));
        assert_eq!(vendors_named("none"), Ok(Vec::new()));
        let wrong = vendors_named("nvidlia").unwrap_err();
        assert!(
            wrong.contains("nvidlia") && wrong.contains("nvidia, amd, intel, apple or none"),
            "{wrong}"
        );
    }

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
