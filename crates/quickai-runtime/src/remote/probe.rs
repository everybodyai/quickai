//! What another computer is (`remote.probe`, WP4.33.4, docs/remote-runs.md section 3): its system and processor, the
//! person's home there, and whether the headless program of this release is there already. One command over SSH asks
//! it of a POSIX shell (Linux, macOS); a computer that does not answer it is asked again as Windows' OpenSSH server's
//! shell, `cmd`, is.

use std::path::Path;
use std::time::Duration;

use super::ssh::{self, sh_quote};
use super::RemoteSpec;

/// How long a question about the computer may take: the connection, the login and a few small commands.
const PROBE_TIMEOUT: Duration = Duration::from_secs(40);

/// The system of a computer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum System {
    Linux,
    Macos,
    Windows,
}

/// What another computer is, as far as running QuickAI there goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Computer {
    pub system: System,
    /// `x86_64` or `aarch64`.
    pub arch: String,
    /// The person's home there, as its shell says it.
    pub home: String,
    /// What the headless program of this release there says of itself (`quickai-run 0.2.0`), when it is there.
    pub headless: Option<String>,
}

impl Computer {
    /// Where the headless program of `version` is kept there, relative to the person's home (the copy of each version
    /// has a name of its own: a program that runs is not written over, which Windows does not allow).
    pub fn program_path(&self, version: &str) -> String {
        match self.system {
            System::Windows => format!(".quickai/bin/quickai-run-{version}.exe"),
            _ => format!(".quickai/bin/quickai-run-{version}"),
        }
    }

    /// The headless program of `version` as its shell runs it.
    pub fn program_command(&self, version: &str) -> String {
        self.shell_path(&self.program_path(version))
    }

    /// `relative` (a path below the person's home, with `/`) as one word of its shell's command line.
    pub fn shell_path(&self, relative: &str) -> String {
        match self.system {
            System::Windows => format!(r#""%USERPROFILE%\{}""#, relative.replace('/', "\\")),
            _ => format!("\"$HOME\"/{}", sh_quote(relative)),
        }
    }

    /// The file of a package's `remote/` folder that is the headless program for this computer, when there is one.
    pub fn package_file(&self) -> Option<&'static str> {
        match (self.system, self.arch.as_str()) {
            (System::Linux, "x86_64") => Some("quickai-run-linux-x86_64"),
            (System::Macos, _) => Some("quickai-run-macos-universal"),
            (System::Windows, "x86_64") => Some("quickai-run-windows-x86_64.exe"),
            _ => None,
        }
    }

    /// The computer in words: `Linux on x86_64`.
    pub fn describe(&self) -> String {
        let system = match self.system {
            System::Linux => "Linux",
            System::Macos => "macOS",
            System::Windows => "Windows",
        };
        format!("{system} on {}", self.arch)
    }
}

/// Asks `spec` what it is, with ssh (`program`), and whether the headless program of `version` is there.
pub fn probe(program: &Path, spec: &RemoteSpec, version: &str) -> Result<Computer, String> {
    let posix = ssh::run(program, spec, &posix_question(version), PROBE_TIMEOUT)?;
    if posix.ok() {
        if let Some(computer) = read_posix(&posix.lines()) {
            return Ok(computer);
        }
    }
    let windows = ssh::run(program, spec, &windows_question(version), PROBE_TIMEOUT)?;
    read_windows(&windows.lines()).ok_or_else(|| {
        let said = posix.stderr.trim();
        format!(
            "{} answered, but not as Linux, macOS or Windows do{}",
            spec.host.trim(),
            if said.is_empty() {
                ".".to_string()
            } else {
                format!(": {said}")
            }
        )
    })
}

/// The question for a POSIX shell: the system and the processor, the home, and the headless program of `version`.
fn posix_question(version: &str) -> String {
    let program = format!("\"$HOME\"/.quickai/bin/quickai-run-{version}");
    format!(
        "uname -sm; printf '%s\\n' \"$HOME\"; if [ -x {program} ]; then {program} --version; else echo none; fi"
    )
}

/// The same for Windows' `cmd` (what PowerShell is given passes it to `cmd` as it is).
fn windows_question(version: &str) -> String {
    let program = format!(r"%USERPROFILE%\.quickai\bin\quickai-run-{version}.exe");
    format!(
        r#"cmd /c "ver & echo %PROCESSOR_ARCHITECTURE% & echo %USERPROFILE% & if exist {program} ({program} --version) else (echo none)""#
    )
}

/// `Linux x86_64`, the home, and `quickai-run 0.2.0` or `none`.
fn read_posix(lines: &[&str]) -> Option<Computer> {
    let mut words = lines.first()?.split_whitespace();
    let system = match words.next()? {
        "Linux" => System::Linux,
        "Darwin" => System::Macos,
        _ => return None,
    };
    let arch = arch_word(words.next()?);
    let home = lines.get(1)?.trim().to_string();
    Some(Computer {
        system,
        arch,
        home,
        headless: headless_line(lines.get(2).copied()),
    })
}

/// `Microsoft Windows [Version 10.0.22631.4317]` (after an empty line), `AMD64`, the home, and the program's line.
fn read_windows(lines: &[&str]) -> Option<Computer> {
    let mut rest = lines.iter().map(|l| l.trim()).filter(|l| !l.is_empty());
    if !rest.next()?.starts_with("Microsoft Windows") {
        return None;
    }
    let arch = arch_word(rest.next()?);
    let home = rest.next()?.to_string();
    Some(Computer {
        system: System::Windows,
        arch,
        home,
        headless: headless_line(rest.next()),
    })
}

/// The processor as Rust names it: `x86_64` (`AMD64` on Windows), `aarch64` (`arm64` on macOS, `ARM64` on Windows).
fn arch_word(word: &str) -> String {
    match word.to_ascii_lowercase().as_str() {
        "x86_64" | "amd64" => "x86_64".into(),
        "aarch64" | "arm64" => "aarch64".into(),
        other => other.to_string(),
    }
}

fn headless_line(line: Option<&str>) -> Option<String> {
    line.map(str::trim)
        .filter(|l| l.starts_with("quickai-run "))
        .map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn what_a_posix_shell_says_is_read() {
        let linux = read_posix(&["Linux x86_64", "/home/me", "quickai-run 0.2.0"]).unwrap();
        assert_eq!(linux.system, System::Linux);
        assert_eq!(linux.arch, "x86_64");
        assert_eq!(linux.home, "/home/me");
        assert_eq!(linux.headless.as_deref(), Some("quickai-run 0.2.0"));
        assert_eq!(linux.package_file(), Some("quickai-run-linux-x86_64"));
        assert_eq!(
            linux.program_path("0.2.0"),
            ".quickai/bin/quickai-run-0.2.0"
        );

        let mac = read_posix(&["Darwin arm64", "/Users/me", "none"]).unwrap();
        assert_eq!((mac.system, mac.arch.as_str()), (System::Macos, "aarch64"));
        assert_eq!(mac.headless, None);
        assert_eq!(mac.package_file(), Some("quickai-run-macos-universal"));

        let arm = read_posix(&["Linux aarch64", "/home/pi", "none"]).unwrap();
        assert_eq!(arm.package_file(), None, "no build for Linux on ARM yet");
        assert_eq!(read_posix(&["'uname' is not recognized"]), None);
    }

    #[test]
    fn what_windows_cmd_says_is_read() {
        let win = read_windows(&[
            "",
            "Microsoft Windows [Version 10.0.22631.4317]",
            "AMD64",
            r"C:\Users\me",
            "none",
        ])
        .unwrap();
        assert_eq!((win.system, win.arch.as_str()), (System::Windows, "x86_64"));
        assert_eq!(win.home, r"C:\Users\me");
        assert_eq!(win.package_file(), Some("quickai-run-windows-x86_64.exe"));
        assert_eq!(
            win.program_command("0.2.0"),
            r#""%USERPROFILE%\.quickai\bin\quickai-run-0.2.0.exe""#
        );
        assert_eq!(read_windows(&["Linux x86_64"]), None);
    }
}
