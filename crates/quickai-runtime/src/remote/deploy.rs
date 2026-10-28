//! The headless program, and what it runs, copied to another computer (`remote.deploy`, WP4.33.5, docs/remote-runs.md
//! section 3): the `quickai-run` of this release for that computer's system, from the package's `remote/` folder
//! (section 4), into the person's `~/.quickai/bin` there, checked by its SHA-256 before it takes its name; and a file
//! the run reads (the configuration selected in the window, its environment file) into `~/.quickai/remote`. Nothing
//! is written outside `~/.quickai` there (C123).

use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::Duration;

use sha2::{Digest, Sha256};

use super::probe::{Computer, System};
use super::ssh;
use super::RemoteSpec;

/// How long the copy of the headless program may take (some MB over a slow line).
const COPY_TIMEOUT: Duration = Duration::from_secs(600);

/// How long a small command there may take.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(60);

/// How long the copy of a model file may take: gigabytes over a line at home.
const BIG_COPY_TIMEOUT: Duration = Duration::from_secs(12 * 3600);

/// Where the headless programs for each system are: `QUICKAI_REMOTE_PROGRAMS` when it names a folder (a test, or a
/// build run from its sources), else the package's `remote/` folder (in the macOS app, `Contents/Resources/remote`).
pub fn programs_dir() -> Option<PathBuf> {
    if let Some(dir) = std::env::var_os("QUICKAI_REMOTE_PROGRAMS").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(dir));
    }
    let exe = std::env::current_exe().ok()?;
    let dir = crate::paths::packaging_dir(exe.parent()?).join("remote");
    dir.is_dir().then_some(dir)
}

/// The headless program for `computer` in this package, or why there is none.
pub fn local_program(computer: &Computer) -> Result<PathBuf, String> {
    let Some(name) = computer.package_file() else {
        return Err(format!(
            "QuickAI has no headless program for {} yet.",
            computer.describe()
        ));
    };
    let Some(dir) = programs_dir() else {
        return Err(
            "This QuickAI has no headless programs to copy to another computer (its `remote` folder is missing)."
                .into(),
        );
    };
    let path = dir.join(name);
    if path.is_file() {
        Ok(path)
    } else {
        Err(format!(
            "This QuickAI has no headless program for {} ({} is missing).",
            computer.describe(),
            path.display()
        ))
    }
}

/// Makes sure the headless program of `version` is on `spec` (`computer`, as `probe` found it), copying it there when
/// it is not, with ssh (`ssh`) and scp (`scp`). What runs it there comes back.
pub fn ensure_program(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    version: &str,
) -> Result<String, String> {
    let command = computer.program_command(version);
    if computer.headless.as_deref() == Some(format!("quickai-run {version}").as_str()) {
        return Ok(command);
    }
    let local = local_program(computer)?;
    let want =
        file_sha256(&local).map_err(|e| format!("Could not read {}: {e}", local.display()))?;
    let path = computer.program_path(version);
    let part = format!("{path}.part");
    make_folder(ssh, spec, computer, ".quickai/bin")?;
    ssh::copy(scp, spec, &local, &part, COPY_TIMEOUT)?;
    let got = remote_sha256(ssh, spec, computer, &part)?;
    if got != want {
        let _ = run(ssh, spec, &remove_command(computer, &part));
        return Err(format!(
            "The headless program arrived on {} changed (its SHA-256 is {got}, not {want}); it was removed.",
            spec.host.trim()
        ));
    }
    let finish = match computer.system {
        System::Windows => format!(
            "cmd /c \"move /y {} {}\"",
            computer.shell_path(&part),
            computer.shell_path(&path)
        ),
        _ => format!(
            "chmod 755 {part} && mv -f {part} {path}",
            part = computer.shell_path(&part),
            path = computer.shell_path(&path)
        ),
    };
    run(ssh, spec, &finish)?;
    let said = run(ssh, spec, &format!("{command} --version"))?;
    let line = said.lines().first().map(|l| l.trim().to_string());
    if line.as_deref() != Some(format!("quickai-run {version}").as_str()) {
        return Err(format!(
            "The headless program was copied to {} but does not run there: {}",
            spec.host.trim(),
            first_words(&said)
        ));
    }
    Ok(command)
}

/// Copies `local` to `relative` (below the person's home, with `/`) on `spec`, its folder made first.
pub fn copy_file(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    local: &Path,
    relative: &str,
) -> Result<(), String> {
    copy_within(ssh, scp, spec, computer, local, relative, COPY_TIMEOUT)
}

/// `copy_file` for a file of gigabytes (a model), given hours.
pub fn copy_big_file(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    local: &Path,
    relative: &str,
) -> Result<(), String> {
    copy_within(ssh, scp, spec, computer, local, relative, BIG_COPY_TIMEOUT)
}

fn copy_within(
    ssh: &Path,
    scp: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    local: &Path,
    relative: &str,
    timeout: Duration,
) -> Result<(), String> {
    if let Some((folder, _)) = relative.rsplit_once('/') {
        make_folder(ssh, spec, computer, folder)?;
    }
    ssh::copy(scp, spec, local, relative, timeout)
}

/// Makes `relative` (below the person's home) on `spec`, and the folders above it.
fn make_folder(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    relative: &str,
) -> Result<(), String> {
    let folder = computer.shell_path(relative);
    let command = match computer.system {
        System::Windows => format!("cmd /c \"if not exist {folder} mkdir {folder}\""),
        _ => format!("mkdir -p {folder}"),
    };
    run(ssh, spec, &command).map(|_| ())
}

/// The SHA-256 of `relative` on `spec`, by the tool its system has.
fn remote_sha256(
    ssh: &Path,
    spec: &RemoteSpec,
    computer: &Computer,
    relative: &str,
) -> Result<String, String> {
    let file = computer.shell_path(relative);
    let command = match computer.system {
        System::Linux => format!("sha256sum {file}"),
        System::Macos => format!("shasum -a 256 {file}"),
        System::Windows => format!("certutil -hashfile {file} SHA256"),
    };
    let said = run(ssh, spec, &command)?;
    said.lines()
        .iter()
        .find_map(|line| hex_digest(line))
        .ok_or_else(|| {
            format!(
                "Could not check the copy on {}: {}",
                spec.host.trim(),
                first_words(&said)
            )
        })
}

/// The digest a line of `sha256sum`, `shasum` or `certutil` says, in lower case: its first word, or the whole line
/// once its spaces are taken out (an older `certutil` puts one between each two digits).
fn hex_digest(line: &str) -> Option<String> {
    let is_digest = |word: &str| word.len() == 64 && word.chars().all(|c| c.is_ascii_hexdigit());
    let first = line.split_whitespace().next()?;
    if is_digest(first) {
        return Some(first.to_ascii_lowercase());
    }
    let compact: String = line.chars().filter(|c| !c.is_whitespace()).collect();
    is_digest(&compact).then(|| compact.to_ascii_lowercase())
}

fn remove_command(computer: &Computer, relative: &str) -> String {
    let file = computer.shell_path(relative);
    match computer.system {
        System::Windows => format!("cmd /c \"del /f /q {file}\""),
        _ => format!("rm -f {file}"),
    }
}

/// Runs `command` on `spec`; a command that did not end well is said with what it wrote.
fn run(ssh: &Path, spec: &RemoteSpec, command: &str) -> Result<ssh::Said, String> {
    let said = ssh::run(ssh, spec, command, COMMAND_TIMEOUT)?;
    if said.ok() {
        Ok(said)
    } else {
        Err(format!(
            "A command on {} failed: {}",
            spec.host.trim(),
            first_words(&said)
        ))
    }
}

/// What a command wrote, in a few words for a sentence: its error stream, else its output.
fn first_words(said: &ssh::Said) -> String {
    let text = if said.stderr.trim().is_empty() {
        said.stdout.trim()
    } else {
        said.stderr.trim()
    };
    let line = text.lines().next().unwrap_or("it said nothing");
    line.chars().take(200).collect()
}

/// The SHA-256 of a file here, in lower case.
fn file_sha256(path: &Path) -> std::io::Result<String> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 1 << 16];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }
    Ok(hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_is_read_from_what_each_system_says() {
        let digest = "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08";
        assert_eq!(
            hex_digest(&format!("{digest}  /home/me/.quickai/bin/q.part")).as_deref(),
            Some(digest)
        );
        assert_eq!(hex_digest(&digest.to_uppercase()).as_deref(), Some(digest));
        let spaced: String = digest
            .as_bytes()
            .chunks(2)
            .map(|pair| std::str::from_utf8(pair).unwrap())
            .collect::<Vec<_>>()
            .join(" ");
        assert_eq!(hex_digest(&spaced).as_deref(), Some(digest));
        assert_eq!(hex_digest("SHA256 hash of q.exe.part:"), None);
        assert_eq!(
            hex_digest("CertUtil: -hashfile command completed successfully."),
            None
        );
    }

    #[test]
    fn a_file_here_is_hashed() {
        let dir = crate::testkit::scratch("deploy-hash");
        let file = dir.join("test");
        std::fs::write(&file, b"test").unwrap();
        assert_eq!(
            file_sha256(&file).unwrap(),
            "9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08"
        );
    }
}
