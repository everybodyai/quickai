//! Downloads a llama.cpp release archive (see `llama_releases`) and
//! installs it under `paths::llama_bin_install_dir`: extracts every file in
//! the zip — the server binary needs its shared libraries sitting right
//! next to it, not just itself — then locates the `llama-server`/
//! `llama-server.exe` executable inside so ⚙ Settings can point straight at
//! it. Mirrors `downloader.rs`'s background-thread-plus-channel shape.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;

use crate::paths;

pub enum InstallEvent {
    Progress { downloaded: u64, total: Option<u64> },
    Done(PathBuf),
    Failed(String),
}

pub struct InstallHandle {
    pub rx: Receiver<InstallEvent>,
    pub label: String,
    pub downloaded: u64,
    pub total: Option<u64>,
}

const CHUNK: usize = 256 * 1024;
// How many directory levels deep to look for the server binary inside the
// extracted archive — official releases put it at the top level, but this
// leaves room for a wrapping folder or two without scanning forever.
const MAX_SEARCH_DEPTH: u32 = 4;

/// Walks `dir` looking for a file named `paths::LLAMA_SERVER_NAME`, up to
/// `MAX_SEARCH_DEPTH` levels deep. Best-effort: an unreadable subdirectory
/// is skipped rather than failing the whole search.
fn find_binary(dir: &Path) -> Option<PathBuf> {
    fn walk(dir: &Path, depth: u32) -> Option<PathBuf> {
        let entries = std::fs::read_dir(dir).ok()?;
        let mut subdirs = Vec::new();
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                subdirs.push(path);
            } else if path
                .file_name()
                .is_some_and(|n| n == paths::LLAMA_SERVER_NAME)
            {
                return Some(path);
            }
        }
        if depth == 0 {
            return None;
        }
        subdirs.into_iter().find_map(|d| walk(&d, depth - 1))
    }
    walk(dir, MAX_SEARCH_DEPTH)
}

/// One previously-installed llama-server build, as found on disk by
/// `list_installed` — independent of whatever "Look up llama.cpp builds"
/// currently shows, so a build from an earlier run can still be
/// re-selected (or deleted) after it's aged out of that list.
pub struct InstalledBuild {
    pub label: String,
    pub binary_path: PathBuf,
    /// The whole subdirectory this build lives in — what actually gets
    /// removed on delete, since the binary alone isn't the whole install
    /// (its shared libraries sit right next to it).
    pub install_dir: PathBuf,
    pub size_bytes: u64,
}

/// Scans `paths::llama_bin_root_dir` for every subdirectory that still
/// contains a `llama-server` — each one is a build a previous "Look up
/// llama.cpp builds" install completed. Best-effort: a directory that
/// isn't readable, or doesn't turn out to contain a server binary (e.g. a
/// leftover from an install that failed partway), is silently skipped
/// rather than surfaced as an error. Returned most-recently-installed
/// first (by the install directory's mtime — a release tag isn't a
/// reliably sortable number across every past and future format), with
/// anything whose mtime can't be read sorted last rather than guessed at.
pub fn list_installed() -> Vec<InstalledBuild> {
    let Ok(entries) = std::fs::read_dir(paths::llama_bin_root_dir()) else {
        return Vec::new();
    };
    let mut builds: Vec<InstalledBuild> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
        .filter_map(|install_dir| {
            let binary_path = find_binary(&install_dir)?;
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

/// Total size of every regular file under `dir`, recursively — an
/// approximation of how much disk space deleting `dir` would free, shown
/// alongside each entry `list_installed` returns.
fn dir_size(dir: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    entries
        .flatten()
        .map(|entry| {
            let path = entry.path();
            if path.is_dir() {
                dir_size(&path)
            } else {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
            }
        })
        .sum()
}

/// Permanently deletes a previously-installed build — the whole
/// `install_dir`, not just the binary, since its shared libraries live
/// alongside it. Best-effort: on Windows this fails while the binary is
/// still running (the file is in use), which surfaces as an error message
/// rather than a panic.
pub fn delete_installed(install_dir: &Path) -> Result<(), String> {
    std::fs::remove_dir_all(install_dir)
        .map_err(|e| format!("could not delete {}: {e}", install_dir.display()))
}

/// llama.cpp's releases use `.zip` for Windows assets and `.tar.gz` for
/// everything else — see `llama_releases`'s module doc.
enum ArchiveKind {
    Zip,
    TarGz,
}

impl ArchiveKind {
    fn from_url(url: &str) -> Option<Self> {
        let lower = url.to_ascii_lowercase();
        if lower.ends_with(".tar.gz") {
            Some(ArchiveKind::TarGz)
        } else if lower.ends_with(".zip") {
            Some(ArchiveKind::Zip)
        } else {
            None
        }
    }

    fn extension(&self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::TarGz => "tar.gz",
        }
    }
}

/// llama.cpp's Windows zips are flat (no wrapping directory), but this
/// tolerates one anyway — `extract_unwrapped_root_dir` behaves exactly
/// like a plain extract when there's nothing to unwrap.
fn extract_zip(archive_path: &Path, install_dir: &Path) -> Result<(), String> {
    let file =
        File::open(archive_path).map_err(|e| format!("could not open downloaded archive: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("could not read archive: {e}"))?;
    archive
        .extract_unwrapped_root_dir(install_dir, zip::read::root_dir_common_filter)
        .map_err(|e| format!("could not extract archive: {e}"))
}

/// Unlike the Windows zips, llama.cpp's Linux/macOS tar.gz assets do wrap
/// everything in a single top-level `llama-<tag>/` directory — left as-is
/// rather than unwrapped, since `find_binary`'s recursive search handles
/// the extra nesting fine and `server_manager` resolves shared libraries
/// relative to wherever the binary actually ends up.
fn extract_tar_gz(archive_path: &Path, install_dir: &Path) -> Result<(), String> {
    let file =
        File::open(archive_path).map_err(|e| format!("could not open downloaded archive: {e}"))?;
    tar::Archive::new(flate2::read::GzDecoder::new(file))
        .unpack(install_dir)
        .map_err(|e| format!("could not extract archive: {e}"))
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

/// Derives an install-directory label from an arbitrary download URL —
/// used when the URL is typed/pasted straight into ⚙ Settings' llama-server
/// binary field rather than picked from "Look up llama.cpp builds" (which
/// already has a release tag to combine via `LlamaBuild::install_label`).
/// Just the filename with its archive extension stripped; falls back to a
/// fixed name on a URL with no filename-shaped last segment (e.g. a bare
/// "https://example.com/download").
pub fn label_from_url(url: &str) -> String {
    let name = url.trim_end_matches('/').rsplit('/').next().unwrap_or(url);
    let stem = name
        .strip_suffix(".tar.gz")
        .or_else(|| name.strip_suffix(".zip"))
        .unwrap_or(name);
    if stem.is_empty() {
        "llama-server-build".to_string()
    } else {
        stem.to_string()
    }
}

/// Starts the download+install in a background thread and returns
/// immediately with a handle the UI can poll each frame via
/// `handle.rx.try_recv()` — same shape as `downloader::spawn_download`.
/// `label` should be `LlamaBuild::install_label()`; if that build is
/// already installed (a previous run completed it), this skips straight
/// to `Done` without re-downloading anything.
pub fn spawn_install(
    url: String,
    label: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> InstallHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let install_dir = paths::llama_bin_install_dir(&label);
    let thread_label = label.clone();

    std::thread::spawn(move || {
        let label = thread_label;
        let send = |e: InstallEvent| {
            let _ = tx.send(e);
            wake();
        };

        if let Some(existing) = find_binary(&install_dir) {
            send(InstallEvent::Done(existing));
            return;
        }

        let Some(kind) = ArchiveKind::from_url(&url) else {
            send(InstallEvent::Failed(format!(
                "don't know how to extract {url} (expected a .zip or .tar.gz)"
            )));
            return;
        };

        let parent = match install_dir.parent() {
            Some(p) => p,
            None => {
                send(InstallEvent::Failed(format!(
                    "invalid install directory {}",
                    install_dir.display()
                )));
                return;
            }
        };
        if let Err(e) = std::fs::create_dir_all(parent) {
            send(InstallEvent::Failed(format!(
                "could not create {}: {e}",
                parent.display()
            )));
            return;
        }

        let archive_path = parent.join(format!("{label}.{}.part", kind.extension()));
        if let Err(e) = download_to(&url, &archive_path, &send) {
            let _ = std::fs::remove_file(&archive_path);
            send(InstallEvent::Failed(e));
            return;
        }

        let extract_result = match kind {
            ArchiveKind::Zip => extract_zip(&archive_path, &install_dir),
            ArchiveKind::TarGz => extract_tar_gz(&archive_path, &install_dir),
        };
        let _ = std::fs::remove_file(&archive_path);
        if let Err(e) = extract_result {
            let _ = std::fs::remove_dir_all(&install_dir);
            send(InstallEvent::Failed(e));
            return;
        }

        let Some(binary) = find_binary(&install_dir) else {
            send(InstallEvent::Failed(format!(
                "extracted {}, but no {} was found inside it",
                install_dir.display(),
                paths::LLAMA_SERVER_NAME
            )));
            return;
        };
        if let Err(e) = make_executable(&binary) {
            send(InstallEvent::Failed(format!(
                "extracted {}, but could not make it executable: {e}",
                binary.display()
            )));
            return;
        }

        send(InstallEvent::Done(binary));
    });

    InstallHandle {
        rx,
        label,
        downloaded: 0,
        total: None,
    }
}

/// Streams `url` into `dest`, reporting progress as it goes — the download
/// half of `spawn_install`, split out so the thread body above can `return`
/// out of it early on error without duplicating cleanup.
fn download_to(url: &str, dest: &Path, send: &impl Fn(InstallEvent)) -> Result<(), String> {
    let resp = ureq::get(url)
        .call()
        .map_err(|e| format!("request failed: {e}"))?;
    let body = resp.into_body();
    let total = body.content_length();
    let mut reader = body.into_reader();

    let mut file =
        File::create(dest).map_err(|e| format!("could not create {}: {e}", dest.display()))?;
    let mut buf = vec![0u8; CHUNK];
    let mut downloaded: u64 = 0;
    loop {
        let n = reader
            .read(&mut buf)
            .map_err(|e| format!("download error: {e}"))?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| format!("write error: {e}"))?;
        downloaded += n as u64;
        send(InstallEvent::Progress { downloaded, total });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Writes a plain file nested one directory deep and checks
    /// `find_binary` locates it — real tar.gz releases nest their binary
    /// under a wrapping `llama-<tag>/` directory (see `extract_tar_gz`),
    /// so this exercises that without needing a real release archive.
    #[test]
    fn find_binary_locates_nested_server_executable() {
        let dir = std::env::temp_dir().join(format!("quickai-findbin-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("sub").join(paths::LLAMA_SERVER_NAME), b"stub").unwrap();
        std::fs::write(dir.join("libggml.so"), b"stub").unwrap();

        let found = find_binary(&dir).expect("expected to find the server binary");
        assert_eq!(found, dir.join("sub").join(paths::LLAMA_SERVER_NAME));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn find_binary_none_when_absent() {
        let dir =
            std::env::temp_dir().join(format!("quickai-findbin-empty-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("readme.txt"), b"stub").unwrap();

        assert!(find_binary(&dir).is_none());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dir_size_sums_nested_files() {
        let dir = std::env::temp_dir().join(format!("quickai-dirsize-test-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sub")).unwrap();
        std::fs::write(dir.join("a"), vec![0u8; 10]).unwrap();
        std::fs::write(dir.join("sub").join("b"), vec![0u8; 20]).unwrap();

        assert_eq!(dir_size(&dir), 30);

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `list_installed`/`delete_installed` operate on the real
    /// `paths::llama_bin_root_dir` (matching this repo's convention of
    /// testing against real resources rather than mocks — there's no
    /// injectable root to redirect them to), so this plants a fake build
    /// under a unique test label there rather than a temp directory, and
    /// cleans it up afterward.
    #[test]
    fn list_installed_finds_and_delete_installed_removes_a_build() {
        let label = format!("quickai-list-installed-test-{}", std::process::id());
        let install_dir = paths::llama_bin_install_dir(&label);
        let _ = std::fs::remove_dir_all(&install_dir);
        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(install_dir.join(paths::LLAMA_SERVER_NAME), vec![0u8; 42]).unwrap();
        std::fs::write(install_dir.join("libggml.so"), vec![0u8; 8]).unwrap();

        let found = list_installed()
            .into_iter()
            .find(|b| b.label == label)
            .expect("expected the fake build to be listed");
        assert_eq!(
            found.binary_path,
            install_dir.join(paths::LLAMA_SERVER_NAME)
        );
        assert_eq!(found.size_bytes, 50);

        delete_installed(&found.install_dir).expect("delete should succeed");
        assert!(!install_dir.exists());
        assert!(!list_installed().into_iter().any(|b| b.label == label));
    }

    /// Plants two fake builds a couple of seconds apart (long enough to
    /// clear any filesystem's mtime granularity) and checks the
    /// more-recently-created one sorts first — "latest at the top" in the
    /// ⚙ Settings list this backs.
    #[test]
    fn list_installed_orders_most_recently_installed_first() {
        let older_label = format!("quickai-order-test-older-{}", std::process::id());
        let newer_label = format!("quickai-order-test-newer-{}", std::process::id());
        let older_dir = paths::llama_bin_install_dir(&older_label);
        let newer_dir = paths::llama_bin_install_dir(&newer_label);
        let _ = std::fs::remove_dir_all(&older_dir);
        let _ = std::fs::remove_dir_all(&newer_dir);

        std::fs::create_dir_all(&older_dir).unwrap();
        std::fs::write(older_dir.join(paths::LLAMA_SERVER_NAME), b"stub").unwrap();
        std::thread::sleep(Duration::from_millis(1100));
        std::fs::create_dir_all(&newer_dir).unwrap();
        std::fs::write(newer_dir.join(paths::LLAMA_SERVER_NAME), b"stub").unwrap();

        let builds = list_installed();
        let newer_pos = builds
            .iter()
            .position(|b| b.label == newer_label)
            .expect("newer build should be listed");
        let older_pos = builds
            .iter()
            .position(|b| b.label == older_label)
            .expect("older build should be listed");
        assert!(
            newer_pos < older_pos,
            "expected the more recently installed build to sort first"
        );

        let _ = std::fs::remove_dir_all(&older_dir);
        let _ = std::fs::remove_dir_all(&newer_dir);
    }

    /// Real download+extract against a small real zip (not an actual
    /// llama.cpp release — this exercises the download/extract/locate
    /// mechanics generically), matching this repo's convention of testing
    /// against real network/process resources rather than mocks. GitHub
    /// serves repository zipballs as plain `.zip` files, and one always
    /// contains at least a top-level directory plus files inside it, which
    /// is enough to exercise `extract_unwrapped_root_dir` — it just won't
    /// contain a `llama-server`, so this checks extraction succeeded via
    /// the directory existing rather than via `find_binary`.
    #[test]
    fn downloads_and_extracts_a_real_zip() {
        let label = format!("quickai-install-test-{}", std::process::id());
        let install_dir = paths::llama_bin_install_dir(&label);
        let _ = std::fs::remove_dir_all(&install_dir);

        let handle = spawn_install(
            "https://github.com/ggml-org/llama.cpp/archive/refs/heads/master.zip".to_string(),
            label.clone(),
            || {},
        );

        let deadline = Instant::now() + Duration::from_secs(60);
        let mut result = None;
        while Instant::now() < deadline && result.is_none() {
            if let Ok(event) = handle.rx.recv_timeout(Duration::from_millis(300)) {
                match event {
                    InstallEvent::Done(p) => result = Some(Ok(p)),
                    InstallEvent::Failed(e) => result = Some(Err(e)),
                    InstallEvent::Progress { .. } => {}
                }
            }
        }

        match result {
            // No llama-server in a source zipball — that's the expected,
            // documented failure mode this test is actually checking for.
            Some(Err(e)) => assert!(e.contains("no llama-server")),
            Some(Ok(p)) => panic!("unexpectedly found a server binary at {}", p.display()),
            None => panic!("install did not finish within timeout"),
        }
        assert!(install_dir.is_dir(), "expected extraction to have happened");
        assert!(
            std::fs::read_dir(&install_dir).unwrap().next().is_some(),
            "expected extracted files inside {}",
            install_dir.display()
        );

        let _ = std::fs::remove_dir_all(&install_dir);
    }

    /// Same as `downloads_and_extracts_a_real_zip`, but for the `.tar.gz`
    /// path — this is what llama.cpp's own Linux/macOS release assets use
    /// (see `llama_releases`'s module doc), so it needs its own coverage
    /// rather than assuming the zip path generalises.
    #[test]
    fn downloads_and_extracts_a_real_tar_gz() {
        let label = format!("quickai-install-targz-test-{}", std::process::id());
        let install_dir = paths::llama_bin_install_dir(&label);
        let _ = std::fs::remove_dir_all(&install_dir);

        let handle = spawn_install(
            "https://github.com/ggml-org/llama.cpp/archive/refs/heads/master.tar.gz".to_string(),
            label.clone(),
            || {},
        );

        let deadline = Instant::now() + Duration::from_secs(60);
        let mut result = None;
        while Instant::now() < deadline && result.is_none() {
            if let Ok(event) = handle.rx.recv_timeout(Duration::from_millis(300)) {
                match event {
                    InstallEvent::Done(p) => result = Some(Ok(p)),
                    InstallEvent::Failed(e) => result = Some(Err(e)),
                    InstallEvent::Progress { .. } => {}
                }
            }
        }

        match result {
            Some(Err(e)) => assert!(e.contains("no llama-server")),
            Some(Ok(p)) => panic!("unexpectedly found a server binary at {}", p.display()),
            None => panic!("install did not finish within timeout"),
        }
        assert!(install_dir.is_dir(), "expected extraction to have happened");
        assert!(
            std::fs::read_dir(&install_dir).unwrap().next().is_some(),
            "expected extracted files inside {}",
            install_dir.display()
        );

        let _ = std::fs::remove_dir_all(&install_dir);
    }

    #[test]
    fn label_from_url_strips_archive_extension() {
        assert_eq!(
            label_from_url("https://example.com/x/llama-b1-bin-ubuntu-vulkan-x64.tar.gz"),
            "llama-b1-bin-ubuntu-vulkan-x64"
        );
        assert_eq!(
            label_from_url("https://example.com/x/llama-b1-bin-win-vulkan-x64.zip"),
            "llama-b1-bin-win-vulkan-x64"
        );
        assert_eq!(label_from_url("https://example.com/download"), "download");
        assert_eq!(
            label_from_url("https://example.com/.tar.gz"),
            "llama-server-build"
        );
    }

    #[test]
    fn archive_kind_from_url_recognises_extensions() {
        assert!(matches!(
            ArchiveKind::from_url("https://example.com/x.zip"),
            Some(ArchiveKind::Zip)
        ));
        assert!(matches!(
            ArchiveKind::from_url("https://example.com/x.tar.gz"),
            Some(ArchiveKind::TarGz)
        ));
        assert!(ArchiveKind::from_url("https://example.com/x.rar").is_none());
    }
}
