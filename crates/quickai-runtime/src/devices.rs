//! The devices llama.cpp can put a model on, as its server lists them (`llama-server --list-devices`): each graphics
//! card with its memory, and whether two NVIDIA cards are linked (NVLink). The panel's Graphics and This computer
//! sections are drawn from them (C213, P1 and G of `the mockups`).
//!
//! The list is what the build of llama.cpp that is installed can use, which is what matters: a CUDA build sees the
//! NVIDIA cards, a Vulkan one every card with a driver, Metal the Apple graphics, and an OpenVINO build its device.

use std::path::Path;
use std::process::Command;

const MIB: u64 = 1024 * 1024;

/// What kind of device it is, for what it is offered for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DeviceKind {
    /// A graphics card of its own, with its own memory.
    Discrete,
    /// Graphics built into the processor: slower than a card, and sharing the computer's memory. Not used unless asked.
    Integrated,
    /// Apple's graphics, which share the computer's memory and are what a model runs on there.
    Unified,
    /// Not graphics at all: the processor under another name (an OpenVINO build's CPU device, BLAS, a software
    /// Vulkan driver).
    Processor,
}

/// One device of the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Device {
    /// llama.cpp's name for it, as `--device` takes it (`CUDA0`, `Vulkan1`, `MTL0`).
    pub id: String,
    /// What it is, as a person calls it (`RTX 4090`, `Radeon RX 7900 XTX`, `Apple M3 Max`).
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    pub kind: DeviceKind,
}

impl Device {
    /// Whether a model can be put on it: it is graphics.
    pub fn is_graphics(&self) -> bool {
        self.kind != DeviceKind::Processor
    }
}

/// A device an engine that finds them itself names in a line of its log (`[devices]` of its document, C213: Ollama's
/// `inference compute` lines), the `n`th it named. One of a library its document says is no card (`cpu`) is the
/// processor: it says the engine found what it has, and that it is not graphics.
pub fn from_log_line(
    spec: &quickai_core::registry::engine::CompiledDevicesLog,
    line: &str,
    n: usize,
) -> Option<Device> {
    let found = spec.log.iter().find_map(|re| re.captures(line))?;
    let library = found
        .name("library")
        .map(|m| m.as_str().trim_matches('"').to_string())
        .unwrap_or_default();
    let name = found
        .name("name")?
        .as_str()
        .trim()
        .trim_matches('"')
        .to_string();
    let total = bytes_of(found.name("total")?.as_str())?;
    let free = found
        .name("available")
        .and_then(|m| bytes_of(m.as_str()))
        .unwrap_or(total);
    let id = format!("{library}{n}");
    let kind = if spec.not_cards.contains(&library.to_lowercase()) {
        DeviceKind::Processor
    } else {
        kind_of(&id, &name)
    };
    Some(Device {
        id,
        name: short_name(&name),
        total_bytes: total,
        free_bytes: free,
        kind,
    })
}

/// `12.0 GiB`, `512 MiB` in bytes.
fn bytes_of(text: &str) -> Option<u64> {
    let text = text.trim().trim_matches('"');
    let (number, unit) = text.split_once(' ').unwrap_or((text, "B"));
    let number: f64 = number.trim().parse().ok()?;
    let scale = match unit.trim().to_ascii_lowercase().as_str() {
        "b" => 1.0,
        "kib" | "kb" => 1024.0,
        "mib" | "mb" => 1024.0 * 1024.0,
        "gib" | "gb" => 1024.0 * 1024.0 * 1024.0,
        "tib" | "tb" => 1024.0 * 1024.0 * 1024.0 * 1024.0,
        _ => return None,
    };
    Some((number * scale) as u64)
}

/// The devices `llama-server --list-devices` printed. Cards of the same name are told apart by their number in the list
/// (`RTX 4090 (1)`, `RTX 4090 (2)`).
pub fn parse(text: &str) -> Vec<Device> {
    let mut devices: Vec<Device> = text
        .lines()
        .skip_while(|l| !l.trim_start().starts_with("Available devices"))
        .skip(1)
        .filter_map(parse_line)
        .collect();
    let names: Vec<String> = devices.iter().map(|d| d.name.clone()).collect();
    for (i, device) in devices.iter_mut().enumerate() {
        if names.iter().filter(|n| **n == names[i]).count() > 1 {
            let nth = names[..=i].iter().filter(|n| **n == names[i]).count();
            device.name = format!("{} ({nth})", names[i]);
        }
    }
    devices
}

/// `  CUDA0: NVIDIA GeForce RTX 4090 (24210 MiB, 23608 MiB free)`
fn parse_line(line: &str) -> Option<Device> {
    let line = line.trim();
    let (id, rest) = line.split_once(": ")?;
    if id.is_empty() || id.contains(' ') {
        return None;
    }
    let rest = rest.strip_suffix(" MiB free)")?;
    let open = rest.rfind(" (")?;
    let (described, numbers) = (&rest[..open], &rest[open + 2..]);
    let (total, free) = numbers.split_once(" MiB, ")?;
    let total: u64 = total.trim().parse().ok()?;
    let free: u64 = free.trim().parse().ok()?;
    let kind = kind_of(id, described);
    Some(Device {
        id: id.to_string(),
        name: short_name(described),
        total_bytes: total * MIB,
        free_bytes: free * MIB,
        kind,
    })
}

/// The kind of a device, from llama.cpp's name for it and what it says it is.
fn kind_of(id: &str, described: &str) -> DeviceKind {
    let upper = id.to_ascii_uppercase();
    let lower = described.to_ascii_lowercase();
    if upper.starts_with("CPU")
        || upper.starts_with("BLAS")
        || lower.contains("device=cpu")
        || lower.contains("llvmpipe")
        || lower.contains("lavapipe")
        || lower.contains("swiftshader")
    {
        return DeviceKind::Processor;
    }
    if upper.starts_with("MTL") || upper.starts_with("METAL") || lower.starts_with("apple") {
        return DeviceKind::Unified;
    }
    const BUILT_IN: [&str; 22] = [
        "uhd graphics",
        "hd graphics",
        "iris",
        "intel(r) graphics",
        "intel graphics",
        "arc(tm) graphics",
        "arc graphics",
        "radeon(tm) graphics",
        "radeon graphics",
        "vega 3 ",
        "vega 6 ",
        "vega 8 ",
        "radeon 610m",
        "radeon 660m",
        "radeon 680m",
        "radeon 740m",
        "radeon 760m",
        "radeon 780m",
        "radeon 880m",
        "radeon 890m",
        "8060s",
        "8050s",
    ];
    if BUILT_IN.iter().any(|w| lower.contains(w)) {
        DeviceKind::Integrated
    } else {
        DeviceKind::Discrete
    }
}

/// What a person calls a device: the maker's words, the marks and the driver's notes left out
/// (`NVIDIA GeForce RTX 4090` is `RTX 4090`, `AMD Radeon RX 7900 XTX (RADV NAVI31)` is `Radeon RX 7900 XTX`).
pub fn short_name(described: &str) -> String {
    // an OpenVINO build says which device it was given before its name
    let described = described
        .rsplit_once(" - ")
        .map_or(described, |(_, name)| name);
    let mut name = described
        .replace("(R)", "")
        .replace("(TM)", "")
        .replace("(tm)", "");
    // the driver's note at the end, in brackets
    if let Some(open) = name.rfind(" (") {
        if name.ends_with(')') {
            name.truncate(open);
        }
    }
    for maker in ["NVIDIA GeForce ", "NVIDIA ", "AMD ", "Intel ", "Apple "] {
        if let Some(rest) = name.strip_prefix(maker) {
            name = if maker == "Apple " {
                format!("Apple {rest}")
            } else {
                rest.to_string()
            };
            break;
        }
    }
    name.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// How long a program is given to list its devices: a build that does not know the option, or that starts a server
/// instead, is stopped then.
const LIST_TIME: std::time::Duration = std::time::Duration::from_secs(15);

/// What `command` writes (its output, then its errors), when it ends within `time`; it is stopped if it does not.
fn output_within(mut command: Command, time: std::time::Duration) -> std::io::Result<String> {
    use std::io::Read;
    command
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped());
    let mut child = command.spawn()?;
    // read as it is written, so that a full pipe does not hold the program
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let reader = std::thread::spawn(move || {
        let mut text = String::new();
        if let Some(out) = out.as_mut() {
            let _ = out.read_to_string(&mut text);
        }
        let mut errors = String::new();
        if let Some(err) = err.as_mut() {
            let _ = err.read_to_string(&mut errors);
        }
        format!("{text}\n{errors}")
    });
    let deadline = std::time::Instant::now() + time;
    loop {
        if child.try_wait()?.is_some() {
            break;
        }
        if std::time::Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            return Err(std::io::Error::new(
                std::io::ErrorKind::TimedOut,
                "it did not list its devices in time",
            ));
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    Ok(reader.join().unwrap_or_default())
}

/// The devices of the llama.cpp server at `binary`, with whether its NVIDIA cards are linked.
pub fn list(binary: &Path) -> Result<(Vec<Device>, bool), String> {
    let mut command = Command::new(binary);
    command.arg("--list-devices");
    // its libraries are beside it, as the engine's document has them found when it starts
    if let Some(dir) = binary.parent() {
        let mut path = std::ffi::OsString::from(dir);
        if let Some(old) = std::env::var_os("LD_LIBRARY_PATH").filter(|o| !o.is_empty()) {
            path.push(":");
            path.push(old);
        }
        command.env("LD_LIBRARY_PATH", path);
        command.current_dir(dir);
    }
    no_console(&mut command);
    let text = output_within(command, LIST_TIME)
        .map_err(|e| format!("{} could not be run: {e}", binary.display()))?;
    let devices = parse(&text);
    let cuda = devices.iter().filter(|d| d.id.starts_with("CUDA")).count();
    let linked = cuda >= 2 && nvlinked();
    Ok((devices, linked))
}

/// On Windows, a program run to ask it something opens no console window.
fn no_console(command: &mut Command) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(crate::runtime::process::windows_creation_flags());
    }
    #[cfg(not(windows))]
    let _ = command;
}

/// Whether `nvidia-smi topo -m` shows two of the cards joined by NVLink (`NV4` where a row meets a column).
fn nvlinked() -> bool {
    let mut command = Command::new("nvidia-smi");
    command.args(["topo", "-m"]);
    no_console(&mut command);
    let Ok(out) = command.output() else {
        return false;
    };
    linked_in(&String::from_utf8_lossy(&out.stdout))
}

/// Whether the matrix `nvidia-smi topo -m` printed has an NVLink between two GPUs.
pub fn linked_in(matrix: &str) -> bool {
    matrix
        .lines()
        .filter(|l| l.trim_start().starts_with("GPU"))
        .flat_map(str::split_whitespace)
        .any(|cell| {
            cell.strip_prefix("NV")
                .is_some_and(|n| !n.is_empty() && n.chars().all(|c| c.is_ascii_digit()))
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cuda_cards_are_read_with_their_memory_and_short_names() {
        let text = "ggml_cuda_init: found 2 CUDA devices:\n\
                    Available devices:\n  \
                    CUDA0: NVIDIA GeForce RTX 4090 (24210 MiB, 23608 MiB free)\n  \
                    CUDA1: NVIDIA GeForce RTX 3060 (12044 MiB, 11800 MiB free)\n";
        let devices = parse(text);
        assert_eq!(devices.len(), 2);
        assert_eq!(devices[0].id, "CUDA0");
        assert_eq!(devices[0].name, "RTX 4090");
        assert_eq!(devices[0].total_bytes, 24210 * MIB);
        assert_eq!(devices[0].free_bytes, 23608 * MIB);
        assert_eq!(devices[0].kind, DeviceKind::Discrete);
        assert_eq!(devices[1].name, "RTX 3060");
    }

    #[test]
    fn a_vulkan_list_tells_built_in_graphics_and_software_from_a_card() {
        let text = "Available devices:\n  \
                    Vulkan0: AMD Radeon RX 7900 XTX (RADV NAVI31) (24560 MiB, 24000 MiB free)\n  \
                    Vulkan1: Intel(R) UHD Graphics 630 (CML GT2) (15646 MiB, 15646 MiB free)\n  \
                    Vulkan2: llvmpipe (LLVM 19.1.1, 256 bits) (31290 MiB, 31290 MiB free)\n";
        let devices = parse(text);
        assert_eq!(devices.len(), 3);
        assert_eq!(devices[0].name, "Radeon RX 7900 XTX");
        assert_eq!(devices[0].kind, DeviceKind::Discrete);
        assert_eq!(devices[1].name, "UHD Graphics 630");
        assert_eq!(devices[1].kind, DeviceKind::Integrated);
        assert_eq!(devices[2].kind, DeviceKind::Processor);
    }

    #[test]
    fn an_openvino_build_on_the_processor_has_no_graphics() {
        let text = "Available devices:\n  OPENVINO0: GGML_OPENVINO_DEVICE=CPU (selected) - Intel(R) Core(TM) \
                    i7-10700 CPU @ 2.90GHz (31290 MiB, 31290 MiB free)\n";
        let devices = parse(text);
        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].kind, DeviceKind::Processor);
        assert!(!devices[0].is_graphics());
        assert_eq!(devices[0].name, "Core i7-10700 CPU @ 2.90GHz");
    }

    #[test]
    fn apple_graphics_share_the_memory() {
        let devices =
            parse("Available devices:\n  MTL0: Apple M3 Max (49152 MiB, 49152 MiB free)\n");
        assert_eq!(devices[0].kind, DeviceKind::Unified);
        assert_eq!(devices[0].name, "Apple M3 Max");
    }

    #[test]
    fn cards_of_the_same_name_are_numbered() {
        let text = "Available devices:\n  \
                    CUDA0: NVIDIA GeForce RTX 3090 (24576 MiB, 24000 MiB free)\n  \
                    CUDA1: NVIDIA GeForce RTX 3090 (24576 MiB, 24000 MiB free)\n  \
                    CUDA2: NVIDIA GeForce RTX 3060 (12044 MiB, 11800 MiB free)\n";
        let names: Vec<String> = parse(text).into_iter().map(|d| d.name).collect();
        assert_eq!(names, ["RTX 3090 (1)", "RTX 3090 (2)", "RTX 3060"]);
    }

    #[test]
    fn nvlink_is_seen_where_two_gpus_meet() {
        let linked = "\tGPU0\tGPU1\tCPU Affinity\nGPU0\t X \tNV4\t0-31\nGPU1\tNV4\t X \t0-31\n";
        assert!(linked_in(linked));
        let not = "\tGPU0\tGPU1\nGPU0\t X \tPHB\nGPU1\tPHB\t X \n";
        assert!(!linked_in(not));
    }
}
