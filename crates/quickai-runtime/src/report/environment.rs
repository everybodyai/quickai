//! The build and the computer, as a problem report says them (`quickai_core::report::Environment`): the version and
//! how the build was made, how QuickAI was installed, the system, the processor's architecture, the memory in steps and
//! the makers of the graphics adapters. Nothing that tells one computer from another: no name, no id, no language.

use std::path::Path;

use quickai_core::report::{measure, Environment};

use crate::gpu::GpuVendor;

/// This build on this computer, whose graphics adapters' makers are already known (`MachineFacts::gpu_vendors`, by
/// their keys: `nvidia`, `amd`, `intel`, `apple`).
pub fn detect(total_ram_bytes: u64, gpu_makers: &[String]) -> Environment {
    let exe = std::env::current_exe().unwrap_or_default();
    let env = |key: &str| std::env::var(key).ok();
    let mut makers: Vec<GpuVendor> = gpu_makers
        .iter()
        .filter_map(|m| GpuVendor::from_key(m))
        .collect();
    makers.sort_by_key(|m| m.key());
    makers.dedup();
    let plain = |v: Option<&'static str>| {
        v.filter(|v| measure::is_plain_name(v) && !v.is_empty())
            .map(str::to_string)
    };
    Environment {
        version: env!("CARGO_PKG_VERSION").to_string(),
        released: option_env!("QUICKAI_CHANNEL") == Some("official"),
        commit: plain(option_env!("QUICKAI_COMMIT")),
        target: plain(option_env!("QUICKAI_TARGET")),
        package: package_words(package(&exe, &env, std::env::consts::OS)).to_string(),
        os: os_name(std::env::consts::OS).to_string(),
        os_version: os_version(sysinfo::System::os_version()),
        arch: std::env::consts::ARCH.to_string(),
        memory_gb: measure::memory_bucket_gb(total_ram_bytes),
        graphics: makers.iter().map(|m| m.label().to_string()).collect(),
    }
}

/// The package of `package` in words.
pub fn package_words(package: &str) -> &'static str {
    match package {
        "appimage" => "AppImage",
        "snap" => "Snap",
        "msix" => "Microsoft Store",
        "brew" => "Homebrew",
        "scoop" => "Scoop",
        "choco" => "Chocolatey",
        "dmg" => "DMG",
        "zip" => "zip",
        "tar" => "tar.gz",
        _ => "other",
    }
}

/// How QuickAI was installed, from where its program is and what the package sets in its environment: `appimage`,
/// `snap`, `msix`, `brew`, `scoop`, `choco`, `dmg`, `zip`, `tar` or `other` (`package_words` says it).
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

/// This computer's name, to take out of a report's text.
pub fn host_name() -> Option<String> {
    sysinfo::System::host_name()
}

fn os_name(os: &str) -> &'static str {
    match os {
        "linux" => "Linux",
        "windows" => "Windows",
        "macos" => "macOS",
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
    fn this_build_says_its_version_and_names_the_makers_of_its_graphics_in_words() {
        let environment = detect(
            16 * 1024 * 1024 * 1024,
            &[
                "nvidia".to_string(),
                "amd".to_string(),
                "nvidia".to_string(),
                "nobody".to_string(),
            ],
        );
        assert_eq!(environment.graphics, vec!["AMD", "NVIDIA"]);
        assert_eq!(environment.version, env!("CARGO_PKG_VERSION"));
        assert_eq!(environment.memory_gb, 16);
        assert_ne!(environment.package, "");
    }

    #[test]
    fn every_package_has_its_words() {
        for package in [
            "appimage", "snap", "msix", "brew", "scoop", "choco", "dmg", "zip", "tar",
        ] {
            assert_ne!(package_words(package), "other", "{package}");
        }
        assert_eq!(package_words("anything"), "other");
    }
}
