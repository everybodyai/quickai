//! What a computer has for models, as the window of another computer is told it (`quickai-headless computer --json`,
//! C228): its system, its memory and how much of it is free, the makers of its graphics cards, and, once llama.cpp is
//! installed there, each device that build can put a model on, with its memory. The panel of the window draws the
//! foot of that computer and the fit of the context from it, as it draws This computer's from its own.
//!
//! The names and sizes of the cards are what the installed llama.cpp says (`devices::list`): before a build is
//! installed there only the makers are known (`devices_known` is false).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::config::ConfigDirs;
use crate::devices::{self, Device, DeviceKind};
use crate::launch::{self, MachineFacts};
use crate::prims::install::{self, InstallSpec};

/// What a computer has, as it says it.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ComputerFacts {
    /// The system and the processor (`linux x86_64`).
    pub system: String,
    /// The memory, in bytes.
    pub memory_bytes: u64,
    /// The memory free now, in bytes; 0 when it could not be told.
    #[serde(default)]
    pub memory_free_bytes: u64,
    /// The makers of the graphics cards found (`NVIDIA`).
    #[serde(default)]
    pub vendors: Vec<String>,
    /// An installed llama.cpp build was asked for its devices, and `devices` is its answer.
    #[serde(default)]
    pub devices_known: bool,
    #[serde(default)]
    pub devices: Vec<DeviceFacts>,
    /// Two NVIDIA cards are linked (NVLink), as the build says.
    #[serde(default)]
    pub linked: bool,
}

/// One device the installed llama.cpp can put a model on.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct DeviceFacts {
    /// llama.cpp's name for it (`CUDA0`).
    pub id: String,
    /// What it is, as a person calls it (`RTX 4090`).
    pub name: String,
    pub total_bytes: u64,
    pub free_bytes: u64,
    /// `discrete`, `integrated`, `unified` or `processor`.
    pub kind: String,
}

impl DeviceFacts {
    /// The device as it is said to another computer.
    pub fn from_device(device: &Device) -> Self {
        DeviceFacts {
            id: device.id.clone(),
            name: device.name.clone(),
            total_bytes: device.total_bytes,
            free_bytes: device.free_bytes,
            kind: match device.kind {
                DeviceKind::Discrete => "discrete",
                DeviceKind::Integrated => "integrated",
                DeviceKind::Unified => "unified",
                DeviceKind::Processor => "processor",
            }
            .to_string(),
        }
    }

    /// The device as the window counts with it; a kind it does not know is taken for a card of its own.
    pub fn to_device(&self) -> Device {
        Device {
            id: self.id.clone(),
            name: self.name.clone(),
            total_bytes: self.total_bytes,
            free_bytes: self.free_bytes,
            kind: match self.kind.as_str() {
                "integrated" => DeviceKind::Integrated,
                "unified" => DeviceKind::Unified,
                "processor" => DeviceKind::Processor,
                _ => DeviceKind::Discrete,
            },
        }
    }
}

/// What this computer has: its memory, the makers of its cards, and the devices of the llama.cpp build installed in
/// QuickAI's data folder (or found where its document looks), when there is one.
pub fn gather(dirs: &ConfigDirs) -> ComputerFacts {
    let vendors = crate::gpu::detect();
    let memory_bytes = crate::hardware::total_ram_bytes();
    let mut facts = ComputerFacts {
        system: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        memory_bytes,
        memory_free_bytes: crate::hardware::available_ram_bytes(),
        vendors: vendors.iter().map(|v| v.label().to_string()).collect(),
        ..ComputerFacts::default()
    };
    let Some(binary) = llama_binary(dirs, MachineFacts::detect(memory_bytes, &vendors)) else {
        return facts;
    };
    if let Ok((list, linked)) = devices::list(&binary) {
        facts.devices_known = true;
        facts.devices = list.iter().map(DeviceFacts::from_device).collect();
        facts.linked = linked;
    }
    facts
}

/// The llama.cpp server a run would use: the one its document finds by itself (a package's, one on the path), else
/// the build installed last, as a run's plan chooses (`provision::plan`); `None` when there is none.
fn llama_binary(dirs: &ConfigDirs, machine: MachineFacts) -> Option<PathBuf> {
    let registry = crate::config::build_registry(dirs);
    let engine = registry.engine(crate::settings::DEFAULT_ENGINE)?;
    let data = dirs.data.as_ref().map(|d| d.display().to_string());
    let found = launch::locate_binary_in(
        &registry,
        crate::settings::DEFAULT_ENGINE,
        None,
        data.as_deref(),
    );
    if found.is_file() {
        return Some(found);
    }
    let mut facts = launch::host_facts(Path::new(""), machine, None);
    if let Some(data) = &data {
        facts.data_dir.clone_from(data);
    }
    let spec = InstallSpec::from_engine(engine, &facts)?;
    install::list_installed(&spec)
        .into_iter()
        .map(|build| build.binary_path)
        .find(|path| path.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_device_goes_and_comes_back_the_same() {
        for kind in [
            DeviceKind::Discrete,
            DeviceKind::Integrated,
            DeviceKind::Unified,
            DeviceKind::Processor,
        ] {
            let device = Device {
                id: "CUDA0".into(),
                name: "RTX 4090".into(),
                total_bytes: 24 << 30,
                free_bytes: 22 << 30,
                kind,
            };
            assert_eq!(DeviceFacts::from_device(&device).to_device(), device);
        }
    }

    #[test]
    fn what_an_older_program_leaves_out_is_read_as_not_known() {
        let facts: ComputerFacts =
            serde_json::from_str(r#"{"system":"linux x86_64","memory_bytes":137438953472}"#)
                .unwrap();
        assert_eq!(facts.memory_bytes, 128 << 30);
        assert!(!facts.devices_known);
        assert!(facts.devices.is_empty());
        assert_eq!(facts.memory_free_bytes, 0);
    }

    #[test]
    fn the_facts_are_written_and_read_back() {
        let facts = ComputerFacts {
            system: "linux x86_64".into(),
            memory_bytes: 128 << 30,
            memory_free_bytes: 100 << 30,
            vendors: vec!["NVIDIA".into()],
            devices_known: true,
            devices: vec![DeviceFacts {
                id: "CUDA0".into(),
                name: "RTX 4090".into(),
                total_bytes: 24 << 30,
                free_bytes: 23 << 30,
                kind: "discrete".into(),
            }],
            linked: false,
        };
        let text = serde_json::to_string(&facts).unwrap();
        assert_eq!(serde_json::from_str::<ComputerFacts>(&text).unwrap(), facts);
    }
}
