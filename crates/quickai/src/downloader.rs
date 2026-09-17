//! Gets a single GGUF file into the models directory — either fetched from
//! a URL (typed in by hand, or picked from `remote`'s online lookup), or
//! imported from a local file path already on disk. Streams progress back
//! to the UI the same way `ServerManager` streams logs: a background
//! thread + a channel drained once per frame.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use crossbeam_channel::Receiver;

/// Whether `input` should be fetched over HTTP rather than treated as a
/// local filesystem path. Deliberately just a scheme check — everything
/// else (a bare filename, a relative path, a Windows `C:\...` or UNC
/// `\\server\share\...` path, a Unix absolute path) falls through to the
/// local-file branch in `spawn_download`.
pub fn looks_like_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

pub enum DownloadEvent {
    Progress { downloaded: u64, total: Option<u64> },
    Done(String),
    Failed(String),
}

pub struct DownloadHandle {
    pub rx: Receiver<DownloadEvent>,
    pub label: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    /// "Downloading"/"Importing" — just for the progress label, so a local
    /// file copy doesn't confusingly claim to be a network download.
    pub verb: &'static str,
}

const CHUNK: usize = 256 * 1024;

/// Turns a URL or local path (plus an optional suggested filename, e.g.
/// from a lookup result) into a safe filename inside models_dir: strips any
/// query string and directory components — both `/` (URLs, Unix paths) and
/// `\` (Windows paths) — so a crafted input can't write outside models_dir
/// via `..` or an absolute path.
fn safe_filename(url: &str, hint: Option<&str>) -> String {
    let raw = hint.unwrap_or(url);
    let raw = raw.split(['?', '#']).next().unwrap_or(raw);
    let raw = raw.rsplit(['/', '\\']).next().unwrap_or(raw).trim();
    let cleaned: String = raw
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
        .collect();
    let cleaned = cleaned.trim_start_matches('.').to_string();
    if cleaned.is_empty() {
        "download.gguf".to_string()
    } else {
        cleaned
    }
}

/// Reads from `reader` in chunks, writing into a fresh `part_path` and
/// reporting progress after each chunk. Shared by the HTTP-download and
/// local-file-copy branches of `spawn_download`, which differ only in
/// where `reader` and `total` come from. Cleans up `part_path` on error.
fn stream_to_part(
    reader: &mut impl Read,
    total: Option<u64>,
    part_path: &Path,
    send: &impl Fn(DownloadEvent),
) -> Result<(), ()> {
    let mut file = match std::fs::File::create(part_path) {
        Ok(f) => f,
        Err(e) => {
            send(DownloadEvent::Failed(format!(
                "could not create {}: {e}",
                part_path.display()
            )));
            return Err(());
        }
    };

    let mut buf = vec![0u8; CHUNK];
    let mut downloaded: u64 = 0;
    loop {
        let n = match reader.read(&mut buf) {
            Ok(0) => break,
            Ok(n) => n,
            Err(e) => {
                send(DownloadEvent::Failed(format!("copy error: {e}")));
                let _ = std::fs::remove_file(part_path);
                return Err(());
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            send(DownloadEvent::Failed(format!("write error: {e}")));
            let _ = std::fs::remove_file(part_path);
            return Err(());
        }
        downloaded += n as u64;
        send(DownloadEvent::Progress { downloaded, total });
    }
    Ok(())
}

/// Starts the download (or local-file import) in a background thread and
/// returns immediately with a handle the UI can poll each frame via
/// `handle.rx.try_recv()`. `input` is either a `http(s)://` URL or a local
/// filesystem path (Unix or Windows-style) to an existing `.gguf` file —
/// see `looks_like_url`.
pub fn spawn_download(
    input: String,
    filename_hint: Option<String>,
    models_dir: PathBuf,
    wake: impl Fn() + Send + Sync + 'static,
) -> DownloadHandle {
    let (tx, rx) = crossbeam_channel::unbounded();
    let filename = safe_filename(&input, filename_hint.as_deref());
    let label = filename.clone();
    let from_url = looks_like_url(&input);

    std::thread::spawn(move || {
        let send = |e: DownloadEvent| {
            let _ = tx.send(e);
            wake();
        };

        let final_path = models_dir.join(&filename);
        let part_path = models_dir.join(format!("{filename}.part"));

        // models_dir is the user's writable download dir, which may not
        // exist yet on a fresh machine (unlike the bundled models dir,
        // this one isn't shipped with the package).
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            send(DownloadEvent::Failed(format!(
                "could not create {}: {e}",
                models_dir.display()
            )));
            return;
        }

        if from_url {
            let resp = match ureq::get(&input).call() {
                Ok(r) => r,
                Err(e) => {
                    send(DownloadEvent::Failed(format!("request failed: {e}")));
                    return;
                }
            };

            let body = resp.into_body();
            let total = body.content_length();
            let mut reader = body.into_reader();
            if stream_to_part(&mut reader, total, &part_path, &send).is_err() {
                return;
            }
        } else {
            let source = PathBuf::from(input.trim());
            let metadata = match std::fs::metadata(&source) {
                Ok(m) => m,
                Err(e) => {
                    send(DownloadEvent::Failed(format!("can't read {}: {e}", source.display())));
                    return;
                }
            };
            if !metadata.is_file() {
                send(DownloadEvent::Failed(format!("{} is not a file", source.display())));
                return;
            }

            // Fast path: a hard link costs no extra disk space and is
            // effectively instant regardless of model size, as long as
            // source and models_dir share a filesystem/volume. Falls back
            // to a real (chunked, progress-reporting) copy otherwise —
            // e.g. the file lives on a different drive.
            if std::fs::hard_link(&source, &final_path).is_ok() {
                send(DownloadEvent::Progress {
                    downloaded: metadata.len(),
                    total: Some(metadata.len()),
                });
                send(DownloadEvent::Done(filename));
                return;
            }

            let mut reader = match std::fs::File::open(&source) {
                Ok(f) => f,
                Err(e) => {
                    send(DownloadEvent::Failed(format!("could not open {}: {e}", source.display())));
                    return;
                }
            };
            if stream_to_part(&mut reader, Some(metadata.len()), &part_path, &send).is_err() {
                return;
            }
        }

        if let Err(e) = std::fs::rename(&part_path, &final_path) {
            send(DownloadEvent::Failed(format!(
                "could not finalize {}: {e}",
                final_path.display()
            )));
            return;
        }

        send(DownloadEvent::Done(filename));
    });

    DownloadHandle {
        rx,
        label,
        downloaded: 0,
        total: None,
        verb: if from_url { "Downloading" } else { "Importing" },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    /// Real download against Hugging Face's API (small JSON, not an actual
    /// GGUF — this exercises the streaming/rename mechanics, not the
    /// model itself), matching this repo's convention of testing against
    /// real network/process resources rather than mocks.
    #[test]
    fn downloads_and_renames_into_place() {
        let dir = std::env::temp_dir().join(format!("quickai-dl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let handle = spawn_download(
            "https://huggingface.co/api/models/unsloth/Qwen3-1.7B".to_string(),
            Some("test-download.json".to_string()),
            dir.clone(),
            || {},
        );

        let deadline = Instant::now() + Duration::from_secs(20);
        let mut result = None;
        while Instant::now() < deadline && result.is_none() {
            if let Ok(event) = handle.rx.recv_timeout(Duration::from_millis(200)) {
                match event {
                    DownloadEvent::Done(f) => result = Some(Ok(f)),
                    DownloadEvent::Failed(e) => result = Some(Err(e)),
                    DownloadEvent::Progress { .. } => {}
                }
            }
        }

        let filename = result.expect("download did not finish within timeout").expect("download failed");
        let path = dir.join(&filename);
        assert!(path.is_file(), "expected {} to exist", path.display());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        assert!(!dir.join(format!("{filename}.part")).exists(), "leftover .part file");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn looks_like_url_detects_http_schemes_only() {
        assert!(looks_like_url("https://example.com/model.gguf"));
        assert!(looks_like_url("HTTP://example.com/model.gguf"));
        assert!(!looks_like_url("/home/user/models/model.gguf"));
        assert!(!looks_like_url(r"C:\Users\me\model.gguf"));
        assert!(!looks_like_url(r"\\server\share\model.gguf"));
        assert!(!looks_like_url("model.gguf"));
    }

    /// A local path (not a URL) should be imported by filesystem copy/link
    /// rather than an HTTP request, landing in models_dir under its own
    /// filename with identical contents.
    #[test]
    fn imports_local_file_by_path() {
        let base = std::env::temp_dir().join(format!("quickai-import-test-{}", std::process::id()));
        let source_dir = base.join("source");
        let dest_dir = base.join("dest");
        std::fs::create_dir_all(&source_dir).unwrap();

        let source_path = source_dir.join("my-model.gguf");
        std::fs::write(&source_path, b"fake gguf contents").unwrap();

        let handle = spawn_download(source_path.display().to_string(), None, dest_dir.clone(), || {});
        assert_eq!(handle.verb, "Importing");

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut result = None;
        while Instant::now() < deadline && result.is_none() {
            if let Ok(event) = handle.rx.recv_timeout(Duration::from_millis(200)) {
                match event {
                    DownloadEvent::Done(f) => result = Some(Ok(f)),
                    DownloadEvent::Failed(e) => result = Some(Err(e)),
                    DownloadEvent::Progress { .. } => {}
                }
            }
        }

        let filename = result.expect("import did not finish within timeout").expect("import failed");
        assert_eq!(filename, "my-model.gguf");
        let imported_path = dest_dir.join(&filename);
        assert_eq!(std::fs::read(&imported_path).unwrap(), b"fake gguf contents");
        assert!(!dest_dir.join(format!("{filename}.part")).exists(), "leftover .part file");

        let _ = std::fs::remove_dir_all(&base);
    }
}
