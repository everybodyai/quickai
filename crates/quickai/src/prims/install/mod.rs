//! `install.archive@1`: downloads a release archive and installs it under the engine's root, then
//! finds the executable inside (docs/architecture.md, sections 8 and 11.5). The engine document
//! says where the builds live, what the executable is called and how deep to look for it; this
//! module knows nothing of any one engine. It also lists and deletes the builds installed earlier,
//! so a build that has aged out of the feed can still be selected again.
//!
//! Every file of the archive is extracted, not just the executable: a server needs its shared
//! libraries right next to it. An install is **verified and atomic** (decisions C26 to C28):
//!
//! ```text
//! free space   enough for the download and what it unpacks to, or no request is made
//! download     <root>/<label>.<ext>.part, with its sha256 worked out on the way in
//! verify       the digest the host lists and the one the document pins must agree, and the file must
//!              have it, all before a byte is unpacked
//! unpack       into <root>/<label>.partial/, entry by entry through the filter of `extract.rs`
//! check        the executable is there, is marked executable, and its smoke test passes
//! rename       <label>.partial -> <label>, so a build is there whole or not at all
//! ```
//!
//! What a killed install leaves (`.part`, `.partial`) is never a build and is cleared by the next.

mod extract;
mod zstd;

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use quickai_core::doc::engine::{Install, Verify};
use quickai_core::plan::HostFacts;
use quickai_core::registry::Engine;
use sha2::{Digest, Sha256};

use crate::hardware::format_bytes;
use crate::jobs::{Job, JobEvent};
use crate::runtime::process::{run_captured, without_package_loader_env, RunFailure};
use extract::{extract, ArchiveKind};

const CHUNK: usize = 256 * 1024;

/// How the download and what it unpacks to are told apart from the room it needs: the unpacked
/// size is not known before the download, and the real archives expand to about 1.5 times (the
/// Ollama archive: 1.44 GB to 2.2 GB), so the estimate is 1.6 times plus a margin.
const UNPACKED_NUMERATOR: u64 = 8;
const UNPACKED_DENOMINATOR: u64 = 5;
const SPACE_MARGIN: u64 = 256 * 1024 * 1024;

/// What an install in progress leaves in the root, and nothing else does.
const PARTIAL: &str = ".partial";
const PART: &str = ".part";

/// Where an engine's builds live and how to recognise an installed one.
#[derive(Clone, Debug)]
pub struct InstallSpec {
    /// One directory per build below this.
    pub root: PathBuf,
    /// The executable's file name on this platform (`llama-server.exe` on Windows).
    pub binary: String,
    /// How many directory levels below a build's directory to look for the executable.
    pub search_depth: u32,
    /// Mark the executable as executable (Unix).
    pub chmod: bool,
    /// What a build downloaded from a bare URL is called when the URL has no usable file name.
    pub fallback_label: String,
    /// Whether a download is compared with a digest.
    pub verify: Verify,
    /// What is left out of an unpacked build: globs relative to the build, already chosen for this
    /// machine.
    pub skip: Vec<String>,
    /// A command that must succeed on the unpacked build before it counts as installed.
    pub smoke: Option<Smoke>,
    /// How much room there is at a path. A parameter so that a test can say "none".
    pub free_space: fn(&Path) -> Option<u64>,
}

/// The program (the build's executable) is run with these arguments.
#[derive(Clone, Debug)]
pub struct Smoke {
    pub args: Vec<String>,
    pub timeout: Duration,
}

impl Default for InstallSpec {
    fn default() -> Self {
        InstallSpec {
            root: PathBuf::new(),
            binary: String::new(),
            search_depth: 4,
            chmod: false,
            fallback_label: "build".to_string(),
            verify: Verify::None,
            skip: Vec::new(),
            smoke: None,
            free_space: system_free_space,
        }
    }
}

impl InstallSpec {
    /// From the engine's `[acquire]` section; `None` when it cannot install (no `install`, no
    /// `root`, no executable named).
    pub fn from_engine(engine: &Engine, facts: &HostFacts) -> Option<InstallSpec> {
        let acquire = engine.doc.acquire.as_ref()?;
        let Install::Archive {
            binary,
            chmod,
            verify,
            smoke,
            ..
        } = acquire.install.as_ref()?;
        let binary = binary.as_ref()?;
        let root = quickai_core::acquire::root_dir(engine, facts)?;
        Some(InstallSpec {
            root: PathBuf::from(root),
            binary: binary_file_name(&binary.name),
            search_depth: binary.search_depth,
            chmod: *chmod,
            fallback_label: format!("{}-build", binary.name),
            verify: *verify,
            skip: quickai_core::acquire::skip_globs(engine, facts),
            smoke: smoke.as_ref().map(|command| Smoke {
                args: command.iter().skip(1).cloned().collect(),
                timeout: Duration::from_secs(15),
            }),
            free_space: system_free_space,
        })
    }

    /// The directory a build with this label is installed in.
    pub fn install_dir(&self, label: &str) -> PathBuf {
        self.root.join(label)
    }
}

/// One file to fetch for an install.
#[derive(Clone, Debug, Default)]
pub struct Source {
    pub url: String,
    /// The sha256 the host lists for the file (hex).
    pub host_digest: Option<String>,
    /// The sha256 the engine's document pins for it (hex).
    pub pin: Option<String>,
    /// The size the host lists, for the room check and the progress bar.
    pub size: Option<u64>,
}

impl Source {
    pub fn bare(url: &str) -> Source {
        Source {
            url: url.to_string(),
            ..Default::default()
        }
    }
}

/// What a person asked to install: a build under a label, made of a base archive and, on request,
/// archives unpacked over it.
#[derive(Clone, Debug)]
pub struct InstallRequest {
    pub label: String,
    pub base: Source,
    pub overlays: Vec<Source>,
}

impl InstallRequest {
    pub fn bare(url: &str, label: &str) -> InstallRequest {
        InstallRequest {
            label: label.to_string(),
            base: Source::bare(url),
            overlays: Vec::new(),
        }
    }

    fn sources(&self) -> impl Iterator<Item = &Source> {
        std::iter::once(&self.base).chain(self.overlays.iter())
    }
}

/// `llama-server` on Unix, `llama-server.exe` on Windows.
pub fn binary_file_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// Walks `dir` looking for a file called `name`, up to `depth` levels down. Best-effort: an
/// unreadable subdirectory is skipped rather than failing the whole search.
pub fn find_binary(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
    fn walk(dir: &Path, name: &str, depth: u32) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path.file_name().is_some_and(|n| n == name) {
                return Some(path);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.into_iter().find_map(|d| walk(&d, name, depth - 1))
    }
    walk(dir, name, depth)
}

/// One build installed by an earlier run, as found on disk — independent of whatever the feed
/// currently lists, so it can be selected again (or deleted) after it has aged out of that list.
pub struct InstalledBuild {
    pub label: String,
    pub binary_path: PathBuf,
    /// The whole directory the build lives in: what a delete removes, since the executable alone is
    /// not the install (its shared libraries sit next to it).
    pub install_dir: PathBuf,
    pub size_bytes: u64,
}

/// Whether a name in the root is what an install in progress leaves, and so is not a build.
pub(crate) fn is_leftover(name: &str) -> bool {
    name.ends_with(PART) || name.ends_with(PARTIAL)
}

/// Every directory under the root that still holds the executable, most recently installed first
/// (by the directory's modification time: a release tag is not a reliably sortable number across
/// every past and future format), anything whose time cannot be read last. A directory that is
/// unreadable or holds no executable, and what an install in progress (or one that was killed)
/// leaves, is skipped.
pub fn list_installed(spec: &InstallSpec) -> Vec<InstalledBuild> {
    let Ok(entries) = std::fs::read_dir(&spec.root) else {
        return Vec::new();
    };
    let mut builds: Vec<InstalledBuild> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .is_some_and(|n| !is_leftover(&n.to_string_lossy()))
        })
        .filter_map(|install_dir| {
            let binary_path = find_binary(&install_dir, &spec.binary, spec.search_depth)?;
            let label = install_dir.file_name()?.to_string_lossy().into_owned();
            let size_bytes = dir_size(&install_dir);
            Some(InstalledBuild {
                label,
                binary_path,
                install_dir,
                size_bytes,
            })
        })
        .collect();
    builds.sort_by_key(|b| {
        std::cmp::Reverse(
            std::fs::metadata(&b.install_dir)
                .and_then(|m| m.modified())
                .ok(),
        )
    });
    builds
}

/// Total size of every regular file under `dir`: roughly what deleting it would free.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            // a link counts for itself, not for what it points at (which may be a directory above)
            let Ok(meta) = std::fs::symlink_metadata(&path) else {
                return 0;
            };
            if meta.is_dir() {
                dir_size(&path)
            } else if meta.file_type().is_symlink() {
                0
            } else {
                meta.len()
            }
        })
        .sum()
}

/// Permanently deletes an installed build, shared libraries and all. On Windows this fails while the
/// executable is running (the file is in use), which comes back as a message, not a panic.
pub fn delete_installed(install_dir: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(install_dir)
        .map_err(|e| format!("could not delete {}: {e}", install_dir.display()))
}

#[cfg(unix)]
fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o755))
}

#[cfg(not(unix))]
fn make_executable(_path: &Path) -> std::io::Result<()> {
    Ok(())
}

/// Starts the download and install in a background thread and returns at once with a `Job` the
/// interface polls each frame. `label` names the build's directory; when that directory already
/// holds the executable (an earlier run finished it) the job is done at once, with no download.
/// A bare address has no digest to check: it is installed because the person asked for exactly
/// that file.
pub fn spawn_install(
    spec: InstallSpec,
    url: String,
    label: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<PathBuf> {
    spawn_install_build(spec, InstallRequest::bare(&url, &label), wake)
}

/// As `spawn_install`, for a build the feed listed, with the digests and sizes it knows and the
/// overlays that were chosen.
pub fn spawn_install_build(
    spec: InstallSpec,
    request: InstallRequest,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<PathBuf> {
    let label = request.label.clone();
    Job::spawn(
        label,
        "Installing",
        wake,
        move |send| match install_blocking(&spec, &request, send) {
            Ok(binary) => send(JobEvent::Done(binary)),
            Err(message) => send(JobEvent::Failed(message)),
        },
    )
}

/// What a failed or killed install must not leave behind: removed when it goes out of scope
/// unless the install finished.
struct Leftovers {
    paths: Vec<PathBuf>,
}

impl Drop for Leftovers {
    fn drop(&mut self) {
        for path in &self.paths {
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// The install itself, on the calling thread: what the `Job` runs, and what a test calls to see
/// every event it sends. `send` gets `Progress` and `Status` events; the result is the outcome.
pub fn install_blocking(
    spec: &InstallSpec,
    request: &InstallRequest,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<PathBuf, String> {
    let final_dir = spec.install_dir(&request.label);
    if let Some(existing) = find_binary(&final_dir, &spec.binary, spec.search_depth) {
        return Ok(existing);
    }

    // everything that can be known without the network is settled first
    let mut plan = Vec::new();
    for source in request.sources() {
        let kind = ArchiveKind::from_url(&source.url).ok_or_else(|| {
            format!(
                "don't know how to extract {} ({})",
                source.url,
                ArchiveKind::EXPECTED
            )
        })?;
        plan.push((source, kind, expected_digest(spec, source)?));
    }
    check_room(spec, request)?;

    std::fs::create_dir_all(&spec.root)
        .map_err(|e| format!("could not create {}: {e}", spec.root.display()))?;
    sweep_leftovers(&spec.root);
    let staging = spec.root.join(format!("{}{PARTIAL}", request.label));
    let mut leftovers = Leftovers {
        paths: vec![staging.clone()],
    };
    std::fs::create_dir(&staging)
        .map_err(|e| format!("could not create {}: {e}", staging.display()))?;

    // the progress bar spans every file when the host told us how big each is
    let grand_total: Option<u64> = request
        .sources()
        .map(|s| s.size)
        .sum::<Option<u64>>()
        .filter(|t| *t > 0);
    // An engine that asks for verification gets a log that follows the steps; one that does not
    // (llama.cpp) has the quiet progress line and the one line at the end it always had.
    let chatty = spec.verify == Verify::Digest;
    let mut fetched = 0u64;
    for (source, kind, expected) in &plan {
        let part = spec
            .root
            .join(format!("{}.{}{PART}", request.label, kind.extension()));
        leftovers.paths.push(part.clone());
        let name = source.url.rsplit('/').next().unwrap_or(&source.url);

        let download = download(
            &source.url,
            &part,
            fetched,
            grand_total.or(source.size),
            send,
        )?;
        fetched += download.bytes;
        match expected {
            Some(expected) => {
                if &download.sha256 != expected {
                    return Err(format!(
                        "the download of {name} does not match its checksum: its sha256 is {}… but {}… was expected (it is damaged, or was changed on the way)",
                        &download.sha256[..12],
                        &expected[..12]
                    ));
                }
                send(JobEvent::Status(format!(
                    "Checked {name} against its sha256 ({}…)",
                    &expected[..12]
                )));
            }
            None if spec.verify == Verify::Digest => send(JobEvent::Status(format!(
                "{name} is not verified: no checksum is known for this file (it was asked for by its address), so it is installed as it came"
            ))),
            None => {}
        }

        if chatty {
            send(JobEvent::Status(format!("Unpacking {name}")));
        }
        extract(*kind, &part, &staging, &spec.skip)?;
        let _ = std::fs::remove_file(&part);
    }

    let Some(found) = find_binary(&staging, &spec.binary, spec.search_depth) else {
        return Err(format!(
            "extracted {}, but no {} was found inside it",
            final_dir.display(),
            spec.binary
        ));
    };
    if spec.chmod {
        make_executable(&found).map_err(|e| {
            format!(
                "extracted {}, but could not make it executable: {e}",
                found.display()
            )
        })?;
    }
    if let Some(smoke) = &spec.smoke {
        send(JobEvent::Status("Checking that it runs".to_string()));
        run_smoke(&found, smoke)?;
    }

    // whole at last: make it the build
    let inside = found.strip_prefix(&staging).unwrap_or(&found).to_path_buf();
    if final_dir.exists() {
        // what an install that never finished (before installs were atomic) left behind
        std::fs::remove_dir_all(&final_dir)
            .map_err(|e| format!("could not replace {}: {e}", final_dir.display()))?;
    }
    rename_into_place(&staging, &final_dir)?;
    leftovers.paths.clear();
    Ok(final_dir.join(inside))
}

/// The digest a download must have, if it has to have one. The host's and the document's agree or
/// the file is refused: it changed after it was tested, and which of them is right does not
/// matter.
fn expected_digest(spec: &InstallSpec, source: &Source) -> Result<Option<String>, String> {
    if spec.verify == Verify::None {
        return Ok(None);
    }
    let clean = |digest: &Option<String>| {
        digest
            .as_deref()
            .map(|d| d.trim().trim_start_matches("sha256:").to_ascii_lowercase())
            .filter(|d| !d.is_empty())
    };
    let (host, pin) = (clean(&source.host_digest), clean(&source.pin));
    for digest in [&host, &pin].into_iter().flatten() {
        if digest.len() != 64 || !digest.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("`{digest}` is not a sha256 digest"));
        }
    }
    match (host, pin) {
        (Some(host), Some(pin)) if host != pin => Err(format!(
            "the host lists sha256 {}… for this file but {}… is the one it was tested with: the file changed after it was tested, so it is not installed",
            &host[..12],
            &pin[..12]
        )),
        (Some(digest), _) | (None, Some(digest)) => Ok(Some(digest)),
        (None, None) => Ok(None),
    }
}

/// Refuses an install that would not fit, before anything is asked of the network.
fn check_room(spec: &InstallSpec, request: &InstallRequest) -> Result<(), String> {
    let Some(total) = request.sources().map(|s| s.size).sum::<Option<u64>>() else {
        return Ok(());
    };
    let needed = total + total / UNPACKED_DENOMINATOR * UNPACKED_NUMERATOR + SPACE_MARGIN;
    match (spec.free_space)(&spec.root) {
        Some(free) if free < needed => Err(format!(
            "not enough free space where {} is: about {} is needed (the download and what it unpacks to) and {} is free",
            spec.root.display(),
            format_bytes(needed),
            format_bytes(free)
        )),
        _ => Ok(()),
    }
}

/// What an install in progress leaves in `root` (only one runs at a time): removed before the next
/// starts, so a killed one does not leave gigabytes behind for ever.
fn sweep_leftovers(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if is_leftover(&name) {
            let path = entry.path();
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else {
                let _ = std::fs::remove_file(path);
            }
        }
    }
}

/// Renames the staging directory to the build's. On Windows a virus scanner can hold a file of it
/// for a moment, and the rename is tried again for a second.
fn rename_into_place(staging: &Path, final_dir: &Path) -> Result<(), String> {
    let mut tries = 0;
    loop {
        match std::fs::rename(staging, final_dir) {
            Ok(()) => return Ok(()),
            Err(e) if tries < 10 && cfg!(windows) => {
                tries += 1;
                let _ = e;
                std::thread::sleep(Duration::from_millis(100));
            }
            Err(e) => {
                return Err(format!(
                    "could not move the unpacked build into place at {}: {e}",
                    final_dir.display()
                ))
            }
        }
    }
}

/// Runs the unpacked executable once with the document's arguments: it must exit with success
/// within the time. A program that cannot even do that is not installed.
fn run_smoke(binary: &Path, smoke: &Smoke) -> Result<(), String> {
    let mut command = Command::new(binary);
    command.args(&smoke.args);
    without_package_loader_env(&mut command);
    let ran = match run_captured(&mut command, smoke.timeout) {
        Ok(ran) => ran,
        Err(RunFailure::Start(e)) => {
            return Err(format!(
                "could not run {} to check it: {e}",
                binary.display()
            ))
        }
        Err(RunFailure::TimedOut) => {
            return Err(format!(
                "the unpacked program did not finish its check within {} seconds, so it is not installed",
                smoke.timeout.as_secs()
            ))
        }
        Err(RunFailure::Wait(e)) => {
            return Err(format!("could not wait for the unpacked program: {e}"))
        }
    };
    if ran.status.success() {
        return Ok(());
    }
    let said = format!("{}\n{}", ran.stdout.trim(), ran.stderr.trim());
    let said = said.trim();
    let name = binary.file_name().unwrap_or_default().to_string_lossy();
    Err(format!(
        "the unpacked program failed its check (`{name} {}` ended with {}){}",
        smoke.args.join(" "),
        ran.status,
        if said.is_empty() {
            String::new()
        } else {
            format!(": {}", said.chars().take(400).collect::<String>())
        }
    ))
}

/// What a download came to.
struct Downloaded {
    bytes: u64,
    sha256: String,
}

/// Streams `url` into `dest`, reporting progress as it goes (`before` bytes of the whole job were
/// fetched already) and working out the sha256 of what arrives, so the digest is known the moment
/// the last byte is. A body that ends short of the length the server announced is an error.
fn download(
    url: &str,
    dest: &Path,
    before: u64,
    total: Option<u64>,
    send: &dyn Fn(JobEvent<PathBuf>),
) -> Result<Downloaded, String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let body = resp.into_body();
    let announced = body.content_length();
    let total = total.or(announced.map(|n| before + n));
    let mut reader = body.into_reader();

    let mut file =
        File::create(dest).map_err(|e| format!("could not create {}: {e}", dest.display()))?;
    let mut buf = vec![0u8; CHUNK];
    let mut hasher = Sha256::new();
    let mut bytes: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download error: {e}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .map_err(|e| format!("write error: {e}"))?;
        hasher.update(&buf[..n]);
        bytes += n as u64;
        send(JobEvent::Progress {
            downloaded: before + bytes,
            total,
        });
    }
    if let Some(announced) = announced {
        if bytes != announced {
            return Err(format!(
                "the download stopped early: {bytes} of {announced} bytes arrived"
            ));
        }
    }
    let sha256 = hasher
        .finalize()
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    Ok(Downloaded { bytes, sha256 })
}

/// How much room there is where `path` is (or would be: the nearest directory that exists).
#[cfg(unix)]
pub fn system_free_space(path: &Path) -> Option<u64> {
    use std::os::unix::ffi::OsStrExt;
    let existing = path.ancestors().find(|p| p.exists())?;
    let c_path = std::ffi::CString::new(existing.as_os_str().as_bytes()).ok()?;
    // SAFETY: a valid C string and a zeroed `statvfs`, which the call fills in.
    let mut stat: libc::statvfs = unsafe { std::mem::zeroed() };
    // SAFETY: as above.
    if unsafe { libc::statvfs(c_path.as_ptr(), &mut stat) } != 0 {
        return None;
    }
    #[allow(clippy::unnecessary_cast)] // the field types differ between platforms
    Some(stat.f_bavail as u64 * stat.f_frsize as u64)
}

#[cfg(windows)]
pub fn system_free_space(path: &Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;
    let existing = path.ancestors().find(|p| p.exists())?;
    let wide: Vec<u16> = existing.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut free_to_caller = 0u64;
    // SAFETY: a NUL-terminated wide string and a valid out pointer; the others may be null.
    let ok = unsafe {
        GetDiskFreeSpaceExW(
            wide.as_ptr(),
            &mut free_to_caller,
            std::ptr::null_mut(),
            std::ptr::null_mut(),
        )
    };
    (ok != 0).then_some(free_to_caller)
}

#[cfg(not(any(unix, windows)))]
pub fn system_free_space(_path: &Path) -> Option<u64> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    // ---- fixtures: a root of its own, real archives, a real server on a free port ------------------

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("quickai-install-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir
    }

    fn spec(root: &Path) -> InstallSpec {
        InstallSpec {
            root: root.to_path_buf(),
            binary: binary_file_name("llama-server"),
            search_depth: 4,
            chmod: true,
            fallback_label: "llama-server-build".into(),
            ..Default::default()
        }
    }

    /// A `.zip` holding `files` (path inside, contents).
    fn zip_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        for (path, data) in files {
            writer
                .start_file(*path, zip::write::SimpleFileOptions::default())
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    /// A `.tar.gz` holding `files`; the files are not executable, as a worst case.
    fn tar_gz_of(files: &[(&str, &[u8])]) -> Vec<u8> {
        let mut builder = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        for (path, data) in files {
            let mut header = tar::Header::new_gnu();
            header.set_size(data.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder.append_data(&mut header, path, *data).unwrap();
        }
        builder.into_inner().unwrap().finish().unwrap()
    }

    /// Serves fixed bodies by path on a free local port and counts the requests it gets.
    struct Fake {
        base: String,
        hits: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    }

    impl Fake {
        fn serve(routes: Vec<(&'static str, u16, Vec<u8>)>) -> Fake {
            let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
            let port = server.server_addr().to_ip().unwrap().port();
            let hits = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            let (h, s) = (hits.clone(), stop.clone());
            std::thread::spawn(move || {
                while !s.load(Ordering::Relaxed) {
                    let Ok(Some(request)) = server.recv_timeout(Duration::from_millis(50)) else {
                        continue;
                    };
                    h.fetch_add(1, Ordering::Relaxed);
                    let path = request.url().to_string();
                    let (status, body) = routes
                        .iter()
                        .find(|(p, _, _)| *p == path)
                        .map(|(_, status, body)| (*status, body.clone()))
                        .unwrap_or((404, b"not found".to_vec()));
                    // a real server states the length of a file it serves; so does this one, however large
                    let _ = request.respond(
                        tiny_http::Response::from_data(body)
                            .with_status_code(status)
                            .with_chunked_threshold(usize::MAX),
                    );
                }
            });
            Fake {
                base: format!("http://127.0.0.1:{port}"),
                hits,
                stop,
            }
        }

        fn url(&self, path: &str) -> String {
            format!("{}{path}", self.base)
        }

        fn requests(&self) -> usize {
            self.hits.load(Ordering::Relaxed)
        }
    }

    impl Drop for Fake {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    fn install(spec: &InstallSpec, url: &str, label: &str) -> Result<PathBuf, String> {
        spawn_install(spec.clone(), url.to_string(), label.to_string(), || {})
            .wait(Duration::from_secs(30))
            .expect("the install finished within the timeout")
    }

    const SERVER: &[u8] = b"#!/bin/sh\necho server\n";

    // ---- installing --------------------------------------------------------------------------------

    #[test]
    fn a_flat_zip_is_installed_and_its_executable_found_and_made_executable() {
        let root = scratch("flat");
        let name = binary_file_name("llama-server");
        let fake = Fake::serve(vec![(
            "/b.zip",
            200,
            zip_of(&[(name.as_str(), SERVER), ("libggml.so", b"lib")]),
        )]);
        let path = install(&spec(&root), &fake.url("/b.zip"), "b1-x").unwrap();
        assert_eq!(path, root.join("b1-x").join(&name));
        assert!(
            root.join("b1-x").join("libggml.so").is_file(),
            "the libraries come too"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(&path).unwrap().permissions().mode();
            assert_eq!(mode & 0o111, 0o111, "executable: {mode:o}");
        }
        let leftovers: Vec<_> = std::fs::read_dir(&root)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(leftovers, ["b1-x"], "no .part file is left behind");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_zips_wrapping_directory_is_removed() {
        let root = scratch("wrapped-zip");
        let name = binary_file_name("llama-server");
        let inside = format!("llama-b1/{name}");
        let fake = Fake::serve(vec![(
            "/b.zip",
            200,
            zip_of(&[(inside.as_str(), SERVER), ("llama-b1/libggml.so", b"lib")]),
        )]);
        let path = install(&spec(&root), &fake.url("/b.zip"), "b1").unwrap();
        assert_eq!(
            path,
            root.join("b1").join(&name),
            "unwrapped, as a flat zip would be"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_tar_gz_keeps_its_wrapping_directory_and_the_executable_is_still_found() {
        let root = scratch("wrapped-tar");
        let name = binary_file_name("llama-server");
        let inside = format!("llama-b1/{name}");
        let fake = Fake::serve(vec![(
            "/b.tar.gz",
            200,
            tar_gz_of(&[(inside.as_str(), SERVER), ("llama-b1/libggml.so", b"lib")]),
        )]);
        let path = install(&spec(&root), &fake.url("/b.tar.gz"), "b1").unwrap();
        assert_eq!(path, root.join("b1").join("llama-b1").join(&name));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o111,
                0o111,
                "chmod made it executable although the archive did not"
            );
        }
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn without_chmod_the_executable_is_left_as_extracted() {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let root = scratch("nochmod");
            let fake = Fake::serve(vec![(
                "/b.tar.gz",
                200,
                tar_gz_of(&[("llama-server", SERVER)]),
            )]);
            let mut s = spec(&root);
            s.chmod = false;
            let path = install(&s, &fake.url("/b.tar.gz"), "b1").unwrap();
            assert_eq!(
                std::fs::metadata(&path).unwrap().permissions().mode() & 0o111,
                0
            );
            let _ = std::fs::remove_dir_all(&root);
        }
    }

    #[test]
    fn an_archive_with_no_executable_fails_and_leaves_nothing() {
        let root = scratch("empty");
        let fake = Fake::serve(vec![("/b.zip", 200, zip_of(&[("readme.txt", b"hello")]))]);
        let error = install(&spec(&root), &fake.url("/b.zip"), "b1").unwrap_err();
        assert!(
            error.contains("no llama-server") || error.contains("no llama-server.exe"),
            "{error}"
        );
        assert!(
            !root.join("b1").exists(),
            "an install that finds no executable leaves nothing behind (it used to leave the files)"
        );
        assert!(list_installed(&spec(&root)).is_empty());
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_corrupt_archive_fails_and_leaves_nothing_behind() {
        let root = scratch("corrupt");
        let fake = Fake::serve(vec![
            ("/b.zip", 200, b"this is not a zip".to_vec()),
            ("/b.tar.gz", 200, b"this is not a tarball".to_vec()),
        ]);
        for (path, label) in [("/b.zip", "z"), ("/b.tar.gz", "t")] {
            let error = install(&spec(&root), &fake.url(path), label).unwrap_err();
            assert!(error.contains("could not"), "{path}: {error}");
            assert!(
                !root.join(label).exists(),
                "{path}: the half-made install is removed"
            );
        }
        let leftovers: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert!(leftovers.is_empty(), "no .part files either: {leftovers:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_build_that_is_already_installed_is_not_downloaded_again() {
        let root = scratch("again");
        let name = binary_file_name("llama-server");
        let fake = Fake::serve(vec![("/b.zip", 200, zip_of(&[(name.as_str(), SERVER)]))]);
        let first = install(&spec(&root), &fake.url("/b.zip"), "b1").unwrap();
        assert_eq!(fake.requests(), 1);
        let second = install(&spec(&root), &fake.url("/b.zip"), "b1").unwrap();
        assert_eq!(second, first);
        assert_eq!(fake.requests(), 1, "the second install made no request");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn an_ending_it_cannot_unpack_is_refused_before_any_request() {
        let root = scratch("ending");
        let fake = Fake::serve(vec![("/b.rar", 200, b"x".to_vec())]);
        let error = install(&spec(&root), &fake.url("/b.rar"), "b1").unwrap_err();
        assert!(error.contains("don't know how to extract"), "{error}");
        assert!(
            error.contains("expected a .zip, .tar.gz, .tgz or .tar.zst"),
            "{error}"
        );
        assert_eq!(fake.requests(), 0);
        assert!(!root.exists(), "nothing was created");
    }

    #[test]
    fn a_failed_download_leaves_no_partial_file() {
        let root = scratch("404");
        let fake = Fake::serve(vec![]);
        let error = install(&spec(&root), &fake.url("/missing.zip"), "b1").unwrap_err();
        assert!(error.starts_with("request failed"), "{error}");
        let leftovers: Vec<_> = std::fs::read_dir(&root).unwrap().flatten().collect();
        assert!(leftovers.is_empty(), "{leftovers:?}");
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_executable_must_be_within_the_search_depth() {
        let root = scratch("depth");
        let name = binary_file_name("llama-server");
        let deep = format!("a/b/c/d/{name}");
        let fake = Fake::serve(vec![(
            "/b.tar.gz",
            200,
            tar_gz_of(&[(deep.as_str(), SERVER)]),
        )]);
        let mut shallow = spec(&root);
        shallow.search_depth = 2;
        assert!(install(&shallow, &fake.url("/b.tar.gz"), "shallow")
            .unwrap_err()
            .contains("no llama-server"));
        // 4 levels of directories are reached with a depth of 4
        let found = install(&spec(&root), &fake.url("/b.tar.gz"), "deep").unwrap();
        assert!(
            found.ends_with(format!("a/b/c/d/{name}")),
            "{}",
            found.display()
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn progress_is_reported_with_the_total_from_the_server() {
        let root = scratch("progress");
        std::fs::create_dir_all(&root).unwrap();
        let body = vec![7u8; 600_000];
        let fake = Fake::serve(vec![("/big.zip", 200, body.clone())]);
        let seen = std::sync::Mutex::new(Vec::new());
        let came = download(
            &fake.url("/big.zip"),
            &root.join("big.part"),
            0,
            None,
            &|event| {
                if let JobEvent::Progress { downloaded, total } = event {
                    seen.lock().unwrap().push((downloaded, total));
                }
            },
        )
        .unwrap();
        assert_eq!(came.bytes, 600_000);
        assert_eq!(
            came.sha256.len(),
            64,
            "the digest is worked out on the way in"
        );
        let seen = seen.into_inner().unwrap();
        assert!(
            seen.len() >= 2,
            "a body this size arrives in several reads: {seen:?}"
        );
        assert!(
            seen.windows(2).all(|w| w[0].0 < w[1].0),
            "progress only grows"
        );
        assert_eq!(seen.last().unwrap().0, 600_000);
        assert!(
            seen.iter().all(|(_, total)| *total == Some(600_000)),
            "{seen:?}"
        );
        assert_eq!(std::fs::read(root.join("big.part")).unwrap(), body);
        let _ = std::fs::remove_dir_all(&root);
    }

    // ---- what is installed ---------------------------------------------------------------------------

    #[test]
    fn find_binary_locates_a_nested_executable_and_none_when_absent() {
        let dir = scratch("find");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join("llama-server"), b"stub").unwrap();
        std::fs::write(dir.join("libggml.so"), b"stub").unwrap();
        assert_eq!(
            find_binary(&dir, "llama-server", 4),
            Some(dir.join("sub").join("llama-server"))
        );
        assert_eq!(
            find_binary(&dir, "llama-server", 0),
            None,
            "depth 0 is the top level only"
        );
        assert_eq!(find_binary(&dir, "other", 4), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let dir = scratch("size");
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.join("sub").join("b"), vec![0u8; 20]).unwrap();
        assert_eq!(dir_size(&dir), 30);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn installed_builds_are_listed_with_their_size_and_can_be_deleted() {
        let root = scratch("list");
        let s = spec(&root);
        assert!(list_installed(&s).is_empty(), "no root yet is no builds");
        let dir = s.install_dir("b1-test");
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(&s.binary), vec![0u8; 42]).unwrap();
        std::fs::write(dir.join("libggml.so"), vec![0u8; 8]).unwrap();
        std::fs::create_dir_all(root.join("not-a-build")).unwrap();
        std::fs::write(root.join("a-file"), b"x").unwrap();

        let listed = list_installed(&s);
        assert_eq!(
            listed.len(),
            1,
            "a directory with no executable and a plain file are skipped"
        );
        assert_eq!(listed[0].label, "b1-test");
        assert_eq!(listed[0].binary_path, dir.join(&s.binary));
        assert_eq!(listed[0].size_bytes, 50);

        delete_installed(&listed[0].install_dir).unwrap();
        assert!(!dir.exists());
        assert!(list_installed(&s).is_empty());
        assert!(
            delete_installed(&dir).is_err(),
            "deleting what is not there says so"
        );
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_most_recently_installed_build_is_listed_first() {
        let root = scratch("order");
        let s = spec(&root);
        for label in ["older", "newer"] {
            let dir = s.install_dir(label);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(&s.binary), b"stub").unwrap();
            // long enough to clear any filesystem's modification-time granularity
            std::thread::sleep(Duration::from_millis(1100));
        }
        let labels: Vec<String> = list_installed(&s).into_iter().map(|b| b.label).collect();
        assert_eq!(labels, ["newer", "older"]);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn the_spec_comes_from_the_engines_document() {
        use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
        let doc = r#"schema = 1
kind = "engine"
id = "demo"
requires = { primitives = ["spawn@1"] }
[engine]
name = "Demo"
[launch]
strategies = ["spawn"]
[launch.spawn]
program = "x"
[acquire]
root = "${data}/demo-bin"
install = { use = "archive", binary = { name = "demo-server", search_depth = 2 }, chmod = true }
"#;
        let source = MemorySource::new(&[("embedded:engines/demo.toml", doc)]);
        let registry = quickai_core::registry::Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(
            registry.diagnostics.is_empty(),
            "{:#?}",
            registry.diagnostics
        );
        let facts = HostFacts {
            data_dir: "/home/u/.quickai".into(),
            ..Default::default()
        };
        let spec = InstallSpec::from_engine(registry.engine("demo").unwrap(), &facts).unwrap();
        assert_eq!(spec.root, PathBuf::from("/home/u/.quickai/demo-bin"));
        assert_eq!(spec.binary, binary_file_name("demo-server"));
        assert_eq!(spec.search_depth, 2);
        assert!(spec.chmod);
        assert_eq!(spec.fallback_label, "demo-server-build");
        assert_eq!(
            spec.install_dir("b1"),
            PathBuf::from("/home/u/.quickai/demo-bin/b1")
        );

        // an engine with no install section cannot install
        let none = doc.replace("install = { use = \"archive\", binary = { name = \"demo-server\", search_depth = 2 }, chmod = true }\n", "");
        let source = MemorySource::new(&[("embedded:engines/demo.toml", none.as_str())]);
        let registry = quickai_core::registry::Registry::build(
            load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
            &crate::prims::known(),
        );
        assert!(InstallSpec::from_engine(registry.engine("demo").unwrap(), &facts).is_none());
    }
}
