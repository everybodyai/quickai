//! `install.archive@1`: downloads a release archive and installs it under the engine's root, then
//! finds the executable inside (docs/architecture.md, sections 8 and 11.5). The engine document
//! says where the builds live, what the executable is called and how deep to look for it; this
//! module knows nothing of any one engine. It also lists and deletes the builds installed earlier,
//! so a build that has aged out of the feed can still be selected again.
//!
//! Every file of the archive is extracted, not just the executable: a server needs its shared
//! libraries right next to it.

use std::fs::File;
use std::io::Read;
use std::path::{Path, PathBuf};

use quickai_core::doc::engine::Install;
use quickai_core::plan::HostFacts;
use quickai_core::registry::Engine;

use crate::jobs::{Job, JobEvent};

const CHUNK: usize = 256 * 1024;

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
}

impl InstallSpec {
    /// From the engine's `[acquire]` section; `None` when it cannot install (no `install`, no `root`).
    pub fn from_engine(engine: &Engine, facts: &HostFacts) -> Option<InstallSpec> {
        let acquire = engine.doc.acquire.as_ref()?;
        let Install::Archive { binary, chmod } = acquire.install.as_ref()?;
        let root = quickai_core::acquire::root_dir(engine, facts)?;
        Some(InstallSpec {
            root: PathBuf::from(root),
            binary: binary_file_name(&binary.name),
            search_depth: binary.search_depth,
            chmod: *chmod,
            fallback_label: format!("{}-build", binary.name),
        })
    }

    /// The directory a build with this label is installed in.
    pub fn install_dir(&self, label: &str) -> PathBuf {
        self.root.join(label)
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

/// Every directory under the root that still holds the executable, most recently installed first
/// (by the directory's modification time: a release tag is not a reliably sortable number across
/// every past and future format), anything whose time cannot be read last. A directory that is
/// unreadable or holds no executable (the leftover of an install that failed part-way) is skipped.
pub fn list_installed(spec: &InstallSpec) -> Vec<InstalledBuild> {
    let Ok(entries) = std::fs::read_dir(&spec.root) else {
        return Vec::new();
    };
    let mut builds: Vec<InstalledBuild> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
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
            if path.is_dir() {
                dir_size(&path)
            } else {
                std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0)
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

/// A `.zip` is usually flat, but a wrapping directory is tolerated and removed: unwrapping behaves
/// like a plain extract when there is nothing to unwrap.
fn extract_zip(archive_path: &Path, install_dir: &Path) -> Result<(), String> {
    let file =
        File::open(archive_path).map_err(|e| format!("could not open downloaded archive: {e}"))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("could not read archive: {e}"))?;
    archive
        .extract_unwrapped_root_dir(install_dir, zip::read::root_dir_common_filter)
        .map_err(|e| format!("could not extract archive: {e}"))
}

/// A `.tar.gz` is extracted as it is, wrapping directory included: the search for the executable
/// copes with the extra level, and the engine finds its libraries relative to wherever it ends up.
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

/// Starts the download and install in a background thread and returns at once with a `Job` the
/// interface polls each frame. `label` names the build's directory; when that directory already
/// holds the executable (an earlier run finished it) the job is done at once, with no download.
pub fn spawn_install(
    spec: InstallSpec,
    url: String,
    label: String,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<PathBuf> {
    let install_dir = spec.install_dir(&label);
    let thread_label = label.clone();

    Job::spawn(label, "Installing", wake, move |send| {
        let label = thread_label;

        if let Some(existing) = find_binary(&install_dir, &spec.binary, spec.search_depth) {
            send(JobEvent::Done(existing));
            return;
        }

        let Some(kind) = ArchiveKind::from_url(&url) else {
            send(JobEvent::Failed(format!(
                "don't know how to extract {url} (expected a .zip or .tar.gz)"
            )));
            return;
        };

        let parent = match install_dir.parent() {
            Some(p) => p,
            None => {
                send(JobEvent::Failed(format!(
                    "invalid install directory {}",
                    install_dir.display()
                )));
                return;
            }
        };
        if let Err(e) = std::fs::create_dir_all(parent) {
            send(JobEvent::Failed(format!(
                "could not create {}: {e}",
                parent.display()
            )));
            return;
        }

        let archive_path = parent.join(format!("{label}.{}.part", kind.extension()));
        if let Err(e) = download_to(&url, &archive_path, &send) {
            let _ = std::fs::remove_file(&archive_path);
            send(JobEvent::Failed(e));
            return;
        }

        let extract_result = match kind {
            ArchiveKind::Zip => extract_zip(&archive_path, &install_dir),
            ArchiveKind::TarGz => extract_tar_gz(&archive_path, &install_dir),
        };
        let _ = std::fs::remove_file(&archive_path);
        if let Err(e) = extract_result {
            let _ = std::fs::remove_dir_all(&install_dir);
            send(JobEvent::Failed(e));
            return;
        }

        let Some(binary) = find_binary(&install_dir, &spec.binary, spec.search_depth) else {
            send(JobEvent::Failed(format!(
                "extracted {}, but no {} was found inside it",
                install_dir.display(),
                spec.binary
            )));
            return;
        };
        if spec.chmod {
            if let Err(e) = make_executable(&binary) {
                send(JobEvent::Failed(format!(
                    "extracted {}, but could not make it executable: {e}",
                    binary.display()
                )));
                return;
            }
        }

        send(JobEvent::Done(binary));
    })
}

/// Streams `url` into `dest`, reporting progress as it goes.
fn download_to(url: &str, dest: &Path, send: &dyn Fn(JobEvent<PathBuf>)) -> Result<(), String> {
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
        send(JobEvent::Progress { downloaded, total });
    }
    Ok(())
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
    fn an_archive_with_no_executable_fails_and_the_leftover_is_not_listed() {
        let root = scratch("empty");
        let fake = Fake::serve(vec![("/b.zip", 200, zip_of(&[("readme.txt", b"hello")]))]);
        let error = install(&spec(&root), &fake.url("/b.zip"), "b1").unwrap_err();
        assert!(
            error.contains("no llama-server") || error.contains("no llama-server.exe"),
            "{error}"
        );
        assert!(
            root.join("b1").is_dir(),
            "what was extracted stays, as it always has"
        );
        assert!(
            list_installed(&spec(&root)).is_empty(),
            "but it is not a build"
        );
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
        assert!(error.contains("expected a .zip or .tar.gz"), "{error}");
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
        download_to(&fake.url("/big.zip"), &root.join("big.part"), &|event| {
            if let JobEvent::Progress { downloaded, total } = event {
                seen.lock().unwrap().push((downloaded, total));
            }
        })
        .unwrap();
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
