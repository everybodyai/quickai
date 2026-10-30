//! Who is sending: the build, the computer and how QuickAI was installed. These are the common properties of
//! section 6.1 of docs/reports-and-community.md, without the ids and the level.

use std::path::Path;

use quickai_core::telemetry::{measure, AppIdentity, Common, Level};

use crate::gpu::GpuVendor;

/// The common properties that do not change while the app runs.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identity {
    pub app: AppIdentity,
    pub os: String,
    pub os_version: String,
    pub arch: String,
    pub memory_gb: u32,
    pub gpus: Vec<String>,
    pub language: String,
}

impl Identity {
    /// This build on this computer.
    pub fn detect(total_ram_bytes: u64, gpus: &[GpuVendor]) -> Self {
        let makers: Vec<String> = gpus.iter().map(|v| v.key().to_string()).collect();
        Self::detect_with(total_ram_bytes, &makers)
    }

    /// This build on this computer, whose graphics adapters' makers are already known (`MachineFacts::gpu_vendors`).
    pub fn detect_with(total_ram_bytes: u64, gpu_makers: &[String]) -> Self {
        let exe = std::env::current_exe().unwrap_or_default();
        let env = |key: &str| std::env::var(key).ok();
        let mut makers: Vec<String> = gpu_makers
            .iter()
            .filter(|m| GpuVendor::from_key(m).is_some())
            .cloned()
            .collect();
        makers.sort();
        makers.dedup();
        Self {
            app: build_identity(package(&exe, &env, std::env::consts::OS)),
            os: os_name(std::env::consts::OS).to_string(),
            os_version: os_version(sysinfo::System::os_version()),
            arch: std::env::consts::ARCH.to_string(),
            memory_gb: measure::memory_bucket_gb(total_ram_bytes),
            gpus: makers,
            language: language(&env),
        }
    }

    /// The common properties of a batch from this computer.
    pub fn common(&self, install: &str, session: &str, level: Level) -> Common {
        Common {
            install: install.to_string(),
            session: session.to_string(),
            app: self.app.clone(),
            os: self.os.clone(),
            os_version: self.os_version.clone(),
            arch: self.arch.clone(),
            memory_gb: self.memory_gb,
            gpus: self.gpus.clone(),
            language: self.language.clone(),
            level: level.id().to_string(),
        }
    }

    /// `linux-x86_64-amd-32gb`: the hardware class of this computer (section 6.4).
    pub fn hardware_class(&self) -> String {
        measure::hardware_class(&self.os, &self.arch, &self.gpus, self.memory_gb)
    }
}

/// This build: its version, whether EverybodyAI released it (`QUICKAI_CHANNEL=official` at build time, set by the
/// release scripts of `quickai_base`), its commit (`QUICKAI_COMMIT`) and target, and the package it came in.
pub fn build_identity(package: &str) -> AppIdentity {
    let plain = |v: Option<&'static str>| {
        v.filter(|v| measure::is_plain_name(v))
            .unwrap_or("unknown")
            .to_string()
    };
    AppIdentity {
        version: env!("CARGO_PKG_VERSION").to_string(),
        channel: if option_env!("QUICKAI_CHANNEL") == Some("official") {
            "official"
        } else {
            "source"
        }
        .to_string(),
        commit: plain(option_env!("QUICKAI_COMMIT")),
        target: plain(option_env!("QUICKAI_TARGET")),
        package: package.to_string(),
    }
}

/// How QuickAI was installed, from where its program is and what the package sets in its environment: one of
/// `quickai_core::telemetry::schema::PACKAGES`.
pub fn package(exe: &Path, env: &dyn Fn(&str) -> Option<String>, os: &str) -> &'static str {
    let set = |key: &str| env(key).is_some_and(|v| !v.is_empty());
    let path = exe.to_string_lossy().replace('\\', "/");
    let has = |part: &str| path.contains(part);
    if set("APPIMAGE") {
        "appimage"
    } else if set("SNAP") {
        "snap"
    } else if has("/WindowsApps/") {
        "msix"
    } else if has("/Cellar/") || has("/Caskroom/") || has("/linuxbrew/") {
        "brew"
    } else if has("/scoop/apps/") {
        "scoop"
    } else if has("/chocolatey/lib/") {
        "choco"
    } else if has(".app/Contents/MacOS/") {
        "dmg"
    } else {
        match os {
            "windows" => "zip",
            "linux" => "tar",
            _ => "other",
        }
    }
}

/// This computer's name, to take out of a crash's text.
pub fn host_name() -> Option<String> {
    sysinfo::System::host_name()
}

fn os_name(os: &str) -> &'static str {
    match os {
        "linux" => "linux",
        "windows" => "windows",
        "macos" => "macos",
        _ => "other",
    }
}

/// The system's version as sysinfo reads it (`24.04`, `11 (26100)`, `15.1`), kept to letters, digits, spaces and
/// `._()-`, at most 32 characters.
pub fn os_version(raw: Option<String>) -> String {
    let kept: String = raw
        .unwrap_or_default()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || " ._()-".contains(*c))
        .take(32)
        .collect();
    let kept = kept.trim();
    if kept.is_empty() {
        "unknown".to_string()
    } else {
        kept.to_string()
    }
}

/// The system's language without its region (`en` of `en_GB.UTF-8`), from `LC_ALL`, `LC_MESSAGES` or `LANG`;
/// `unknown` when none says one (Windows sets none of them).
pub fn language(env: &dyn Fn(&str) -> Option<String>) -> String {
    ["LC_ALL", "LC_MESSAGES", "LANG"]
        .iter()
        .filter_map(|key| env(key))
        .map(|v| {
            v.chars()
                .take_while(char::is_ascii_alphabetic)
                .collect::<String>()
                .to_ascii_lowercase()
        })
        .find(|lang| (2..=3).contains(&lang.len()))
        .unwrap_or_else(|| "unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_of(pairs: &'static [(&'static str, &'static str)]) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| v.to_string())
        }
    }

    #[test]
    fn the_package_is_read_from_where_the_program_is_and_what_the_package_sets() {
        let none = env_of(&[]);
        let p = |exe: &str, env: &dyn Fn(&str) -> Option<String>, os: &str| {
            package(Path::new(exe), env, os)
        };
        assert_eq!(
            p(
                "/tmp/.mount_x/usr/bin/quickai",
                &env_of(&[("APPIMAGE", "/home/g/QuickAI.AppImage")]),
                "linux"
            ),
            "appimage"
        );
        assert_eq!(
            p(
                "/snap/quickai/12/bin/quickai",
                &env_of(&[("SNAP", "/snap/quickai/12")]),
                "linux"
            ),
            "snap"
        );
        assert_eq!(
            p(
                r"C:\Program Files\WindowsApps\EverybodyAI.QuickAI_1\quickai.exe",
                &none,
                "windows"
            ),
            "msix"
        );
        assert_eq!(
            p(
                "/opt/homebrew/Caskroom/quickai/0.3.0/QuickAI.app/Contents/MacOS/quickai",
                &none,
                "macos"
            ),
            "brew"
        );
        assert_eq!(
            p(
                r"C:\Users\g\scoop\apps\quickai\current\quickai.exe",
                &none,
                "windows"
            ),
            "scoop"
        );
        assert_eq!(
            p(
                r"C:\ProgramData\chocolatey\lib\quickai\tools\quickai.exe",
                &none,
                "windows"
            ),
            "choco"
        );
        assert_eq!(
            p(
                "/Applications/QuickAI.app/Contents/MacOS/quickai",
                &none,
                "macos"
            ),
            "dmg"
        );
        assert_eq!(p(r"D:\apps\quickai\quickai.exe", &none, "windows"), "zip");
        assert_eq!(p("/home/g/quickai-0.3.0/quickai", &none, "linux"), "tar");
        // an empty variable is no package's
        assert_eq!(
            p(
                "/home/g/quickai/quickai",
                &env_of(&[("APPIMAGE", "")]),
                "linux"
            ),
            "tar"
        );
    }

    #[test]
    fn the_language_is_the_systems_without_its_region() {
        assert_eq!(language(&env_of(&[("LANG", "en_GB.UTF-8")])), "en");
        assert_eq!(
            language(&env_of(&[
                ("LC_ALL", "pt_BR.UTF-8"),
                ("LANG", "en_US.UTF-8")
            ])),
            "pt"
        );
        assert_eq!(language(&env_of(&[("LANG", "C.UTF-8")])), "unknown");
        assert_eq!(language(&env_of(&[])), "unknown");
    }

    #[test]
    fn the_systems_version_is_kept_short_and_plain() {
        assert_eq!(os_version(Some("24.04".into())), "24.04");
        assert_eq!(os_version(Some("11 (26100)".into())), "11 (26100)");
        assert_eq!(
            os_version(Some("rolling\n<script>".into())),
            "rollingscript"
        );
        assert_eq!(os_version(None), "unknown");
        assert!(os_version(Some("9".repeat(80))).len() <= 32);
    }

    #[test]
    fn this_builds_identity_passes_the_schema() {
        let identity = Identity::detect(
            16 * 1024 * 1024 * 1024,
            &[GpuVendor::Nvidia, GpuVendor::Amd],
        );
        assert_eq!(identity.gpus, vec!["amd", "nvidia"]);
        let common = identity.common(
            "3f1c9a2e-7b44-4d0e-9c1a-0b6f1e2a3c4d",
            "5b0e2c7a-1111-4222-8333-444455556666",
            Level::All,
        );
        assert_eq!(
            quickai_core::telemetry::schema::check_common(&common),
            Ok(())
        );
        assert_eq!(identity.app.version, env!("CARGO_PKG_VERSION"));
    }
}
