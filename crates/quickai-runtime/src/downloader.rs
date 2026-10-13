//! Gets a single GGUF file into the models directory — either fetched from
//! a URL (typed in by hand, or picked from `remote`'s online lookup), or
//! imported from a local file path already on disk. Streams progress back
//! to the UI the same way `ServerManager` streams logs: a background
//! thread + a channel drained once per frame.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use quickai_core::lookup::present::{is_place, safe_file_name, Download, Piece};
use sha2::{Digest, Sha256};

use crate::bundles;
use crate::jobs::{Job, JobEvent};

/// Whether `input` should be fetched over HTTP rather than treated as a
/// local filesystem path. Deliberately just a scheme check — everything
/// else (a bare filename, a relative path, a Windows `C:\...` or UNC
/// `\\server\share\...` path, a Unix absolute path) falls through to the
/// local-file branch in `spawn_download`.
pub fn looks_like_url(input: &str) -> bool {
    let lower = input.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// Symlinks `dest` to `source` if this platform supports it (Unix always
/// does; Windows only without extra privilege outside Developer Mode) —
/// `spawn_download`'s preferred way to make an imported local file show up
/// in models_dir, since unlike a hard link it works across filesystem/
/// volume boundaries too.
#[cfg(unix)]
fn try_symlink(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, dest)
}

#[cfg(windows)]
fn try_symlink(source: &Path, dest: &Path) -> std::io::Result<()> {
    std::os::windows::fs::symlink_file(source, dest)
}

#[cfg(not(any(unix, windows)))]
fn try_symlink(_source: &Path, _dest: &Path) -> std::io::Result<()> {
    Err(std::io::Error::new(
        std::io::ErrorKind::Unsupported,
        "symlinks not supported on this platform",
    ))
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
    send: &dyn Fn(JobEvent<String>),
) -> Result<(), ()> {
    stream_to_part_at(reader, 0, total, part_path, send, None).map(|_| ())
}

/// The same, for a file that is one of several: progress is counted from `base` bytes (what the
/// files before it came to) towards `total`, the bytes of all of them. Gives what this one came to.
/// `digest`, when there is one, is fed every byte on its way to the disk, so that a file is checked
/// without being read a second time.
fn stream_to_part_at(
    reader: &mut impl Read,
    base: u64,
    total: Option<u64>,
    part_path: &Path,
    send: &dyn Fn(JobEvent<String>),
    mut digest: Option<&mut Sha256>,
) -> Result<u64, ()> {
    let mut file = match std::fs::File::create(part_path) {
        Ok(f) => f,
        Err(e) => {
            send(JobEvent::Failed(format!(
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
                send(JobEvent::Failed(format!("copy error: {e}")));
                let _ = std::fs::remove_file(part_path);
                return Err(());
            }
        };
        if let Err(e) = file.write_all(&buf[..n]) {
            send(JobEvent::Failed(format!("write error: {e}")));
            let _ = std::fs::remove_file(part_path);
            return Err(());
        }
        if let Some(digest) = digest.as_deref_mut() {
            digest.update(&buf[..n]);
        }
        downloaded += n as u64;
        send(JobEvent::Progress {
            downloaded: base + downloaded,
            total,
        });
    }
    Ok(downloaded)
}

/// Starts the download (or local-file import) in a background thread and
/// returns immediately with a `Job` the UI polls each frame via
/// `Job::poll`. `input` is either a `http(s)://` URL or a local
/// filesystem path (Unix or Windows-style) to an existing `.gguf` file —
/// see `looks_like_url`.
pub fn spawn_download(
    input: String,
    filename_hint: Option<String>,
    models_dir: PathBuf,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<String> {
    let filename = safe_filename(&input, filename_hint.as_deref());
    let label = filename.clone();
    let from_url = looks_like_url(&input);
    let verb = if from_url { "Downloading" } else { "Importing" };

    Job::spawn(label, verb, wake, move |send| {
        let final_path = models_dir.join(&filename);
        let part_path = models_dir.join(format!("{filename}.part"));

        // models_dir is the user's writable download dir, which may not
        // exist yet on a fresh machine (unlike the bundled models dir,
        // this one isn't shipped with the package).
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            send(JobEvent::Failed(format!(
                "could not create {}: {e}",
                models_dir.display()
            )));
            return;
        }

        if from_url {
            let resp = match ureq::get(&input).call() {
                Ok(r) => r,
                Err(e) => {
                    send(JobEvent::Failed(format!("request failed: {e}")));
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
                    send(JobEvent::Failed(format!(
                        "can't read {}: {e}",
                        source.display()
                    )));
                    return;
                }
            };
            if !metadata.is_file() {
                send(JobEvent::Failed(format!(
                    "{} is not a file",
                    source.display()
                )));
                return;
            }

            // Fast path: link rather than copy, so the original file stays
            // exactly where it is (e.g. picked via the "..." browse button
            // from anywhere on disk) and only a link shows up in
            // models_dir — enough for it to appear in the model list,
            // without an extra multi-GB copy. A symlink is tried first
            // since, unlike a hard link, it works across filesystem/volume
            // boundaries; a hard link is tried next since it needs no
            // extra privilege on Windows (symlinks there need Developer
            // Mode or admin) as long as source and models_dir share a
            // volume. Falls back to a real (chunked, progress-reporting)
            // copy only if neither link works.
            let source_abs = std::fs::canonicalize(&source).unwrap_or_else(|_| source.clone());
            if try_symlink(&source_abs, &final_path).is_ok()
                || std::fs::hard_link(&source, &final_path).is_ok()
            {
                send(JobEvent::Progress {
                    downloaded: metadata.len(),
                    total: Some(metadata.len()),
                });
                send(JobEvent::Done(filename));
                return;
            }

            let mut reader = match std::fs::File::open(&source) {
                Ok(f) => f,
                Err(e) => {
                    send(JobEvent::Failed(format!(
                        "could not open {}: {e}",
                        source.display()
                    )));
                    return;
                }
            };
            if stream_to_part(&mut reader, Some(metadata.len()), &part_path, &send).is_err() {
                return;
            }
        }

        if let Err(e) = std::fs::rename(&part_path, &final_path) {
            send(JobEvent::Failed(format!(
                "could not finalize {}: {e}",
                final_path.display()
            )));
            return;
        }

        send(JobEvent::Done(filename));
    })
}

/// The start of a SHA-256, enough to tell two apart in a sentence.
fn short(hash: &str) -> String {
    format!("{}…", hash.chars().take(12).collect::<String>())
}

/// Starts the download of the files of a look-up's button, one after the other, in a background
/// thread: the weights first, then what goes with them. The bar counts the bytes of all of them; a
/// file that is already in the folder at the size it should be is not fetched again (a second
/// quantization of a repository shares its projector); the record that pairs them (`bundles`) is
/// written when the last file is in place. If a file after the weights fails the weights stay, because
/// they run alone, and the message says what was not fetched. The job's result is the weights' name.
pub fn spawn_files(
    download: Download,
    models_dir: PathBuf,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<String> {
    spawn_files_with(download, models_dir, FetchWith::default(), wake)
}

/// What a person who runs the program says about how the files of a download are got (the window says
/// nothing of it): files they already have, which are used where they are, and the logins to send to the
/// sites that ask for one.
#[derive(Clone, Debug, Default)]
pub struct FetchWith {
    /// Files on this computer, by the name each is kept under: checked, then linked into the models folder.
    pub locals: std::collections::BTreeMap<String, PathBuf>,
    /// `(host, token)`: a bearer token is sent to a request for exactly that host and to no other (the sites
    /// a hub sends a file on to are not trusted with it).
    pub logins: Vec<(String, quickai_core::profile::environment::Secret)>,
    /// A size that a file states for itself is the size of what arrives, with or without a checksum (the
    /// window's downloads come from lists whose sizes are approximate in places, and are not held to it).
    pub strict_size: bool,
}

impl FetchWith {
    /// The token to send with a request for `url`, if its host is one a login was given for.
    fn login_for(&self, url: &str) -> Option<&str> {
        let host = quickai_core::profile::split_url_with(url, true)?.1;
        let host = host.split(':').next().unwrap_or(host);
        self.logins
            .iter()
            .find(|(h, _)| h.eq_ignore_ascii_case(host))
            .map(|(_, token)| token.expose())
    }
}

/// What the answer to a request that failed means for whoever asked, when it says something they can do
/// something about.
pub fn explain_status(code: u16, url: &str, has_login: bool) -> Option<String> {
    let host = quickai_core::profile::split_url_with(url, true).map(|(_, host, _)| host)?;
    match (code, has_login) {
        (401, false) => Some(format!(
            "{host} answered 401: the file needs a login. Give a token with --hf-token, the variable HF_TOKEN, or `huggingface_token` of an environment file"
        )),
        (401, true) => Some(format!(
            "{host} answered 401: it did not accept the token it was given"
        )),
        (403, false) => Some(format!(
            "{host} answered 403: the file is not open to everyone. If it asks you to accept a licence, do that on its page, then give a token with --hf-token, the variable HF_TOKEN, or `huggingface_token` of an environment file"
        )),
        (403, true) => Some(format!(
            "{host} answered 403: the token it was given is not allowed to read the file (a licence to accept on its page, or a token that may not read it)"
        )),
        (404, _) => Some(format!("{host} answered 404: there is no such file at that address")),
        _ => None,
    }
}

/// Checks that `source` is the file `piece` should be, then puts it in `final_path` as a link (or, where a
/// link cannot be made, a copy). The bar moves as the checksum is worked out, since that is what takes the
/// time.
fn use_local(
    source: &Path,
    final_path: &Path,
    piece: &Piece,
    base: u64,
    total: Option<u64>,
    send: &dyn Fn(JobEvent<String>),
) -> Result<u64, String> {
    let meta = std::fs::metadata(source)
        .map_err(|e| format!("could not read {}: {e}", source.display()))?;
    if !meta.is_file() {
        return Err(format!("{} is not a file", source.display()));
    }
    if let Some(size) = piece.size.filter(|size| *size != meta.len()) {
        return Err(format!(
            "{} is {} bytes and {} should be {size}: it was not used",
            source.display(),
            meta.len(),
            piece.store_as
        ));
    }
    if let Some(want) = &piece.sha256 {
        send(JobEvent::Status(format!(
            "Checking {} against its checksum.",
            source.display()
        )));
        let mut file = std::fs::File::open(source)
            .map_err(|e| format!("could not read {}: {e}", source.display()))?;
        let mut digest = Sha256::new();
        let mut buf = vec![0u8; CHUNK];
        let mut read_so_far = 0u64;
        loop {
            let n = file
                .read(&mut buf)
                .map_err(|e| format!("could not read {}: {e}", source.display()))?;
            if n == 0 {
                break;
            }
            digest.update(&buf[..n]);
            read_so_far += n as u64;
            send(JobEvent::Progress {
                downloaded: base + read_so_far,
                total,
            });
        }
        let got: String = digest
            .finalize()
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        if got != *want {
            return Err(format!(
                "{} does not match the checksum {} has (expected {}, got {}): it was not used",
                source.display(),
                piece.store_as,
                short(want),
                short(&got)
            ));
        }
    } else {
        send(JobEvent::Progress {
            downloaded: base + meta.len(),
            total,
        });
    }
    // whatever is in the way (a file of the wrong size, an old link) gives place; a folder does not
    if let Ok(existing) = std::fs::symlink_metadata(final_path) {
        if existing.is_dir() {
            return Err(format!("{} is a folder", final_path.display()));
        }
        std::fs::remove_file(final_path)
            .map_err(|e| format!("could not replace {}: {e}", final_path.display()))?;
    }
    let absolute = std::fs::canonicalize(source).unwrap_or_else(|_| source.to_path_buf());
    match try_symlink(&absolute, final_path) {
        Ok(()) => {
            send(JobEvent::Status(format!(
                "{} is {} (linked, not copied).",
                piece.store_as,
                absolute.display()
            )));
        }
        Err(_) => {
            std::fs::copy(source, final_path)
                .map_err(|e| format!("could not copy {}: {e}", source.display()))?;
            send(JobEvent::Status(format!(
                "{} is a copy of {}.",
                piece.store_as,
                source.display()
            )));
        }
    }
    Ok(meta.len())
}

/// As `spawn_files`, with what the person who runs the program says about how the files are got.
pub fn spawn_files_with(
    download: Download,
    models_dir: PathBuf,
    with: FetchWith,
    wake: impl Fn() + Send + Sync + 'static,
) -> Job<String> {
    // the file the job is named after: the weights, or the one file of a download that has none (a VAE
    // fetched on its own)
    let weights = download
        .primary()
        .map(|w| safe_file_name(&w.store_as))
        .unwrap_or_default();
    let more = download.files.len().saturating_sub(1);
    let label = match more {
        0 => weights.clone(),
        1 => format!("{weights} and 1 more file"),
        n => format!("{weights} and {n} more files"),
    };
    Job::spawn(label, "Downloading", wake, move |send| {
        // the folder each file goes in: the models folder, or a folder of it that the engine keeps
        // that kind of file in
        let folder_of = |piece: &Piece| -> Result<PathBuf, String> {
            match piece.place.as_deref() {
                None => Ok(models_dir.clone()),
                Some(place) if is_place(place) => Ok(models_dir.join(place)),
                Some(place) => Err(format!("`{place}` is not the name of a folder")),
            }
        };
        if let Err(e) = std::fs::create_dir_all(&models_dir) {
            send(JobEvent::Failed(format!(
                "could not create {}: {e}",
                models_dir.display()
            )));
            return;
        }
        let total = download.total();
        let count = download.files.len();
        let mut before = 0u64;
        for (index, piece) in download.files.iter().enumerate() {
            let name = safe_file_name(&piece.store_as);
            let folder = match folder_of(piece) {
                Ok(folder) => folder,
                Err(e) => {
                    send(JobEvent::Failed(e));
                    return;
                }
            };
            if let Err(e) = std::fs::create_dir_all(&folder) {
                send(JobEvent::Failed(format!(
                    "could not create {}: {e}",
                    folder.display()
                )));
                return;
            }
            let final_path = folder.join(&name);
            let part_path = folder.join(format!("{name}.part"));
            let failed = |what: String| {
                if index == 0 {
                    JobEvent::Failed(what)
                } else {
                    JobEvent::Failed(format!(
                        "could not fetch {}: {what}; {weights} was fetched and runs without it",
                        piece.name
                    ))
                }
            };
            // already here, and the size it should be: not fetched again
            if let (Some(size), Ok(meta)) = (piece.size, std::fs::metadata(&final_path)) {
                if meta.is_file() && meta.len() == size {
                    send(JobEvent::Status(format!(
                        "{name} is already in the models folder."
                    )));
                    before += size;
                    send(JobEvent::Progress {
                        downloaded: before,
                        total,
                    });
                    continue;
                }
            }
            // a file the person has: used where it is, and checked first
            if let Some(source) = with.locals.get(&piece.store_as) {
                match use_local(source, &final_path, piece, before, total, &|e| send(e)) {
                    Ok(written) => {
                        before += written;
                        continue;
                    }
                    Err(why) => {
                        send(failed(why));
                        return;
                    }
                }
            }
            if count > 1 {
                send(JobEvent::Status(format!(
                    "Downloading {} ({} of {count})",
                    piece.name,
                    index + 1
                )));
            }
            let login = with.login_for(&piece.url);
            let mut request = ureq::get(&piece.url)
                .header("User-Agent", concat!("quickai/", env!("CARGO_PKG_VERSION")));
            if let Some(token) = login {
                request = request.header("Authorization", format!("Bearer {token}"));
            }
            // a login goes on with a redirect inside the host it was given for (a repository that moved),
            // and not to the host a hub sends a file on to
            let response = match request
                .config()
                .redirect_auth_headers(ureq::config::RedirectAuthHeaders::SameHost)
                .build()
                .call()
            {
                Ok(r) => r,
                Err(ureq::Error::StatusCode(code)) => {
                    send(failed(
                        explain_status(code, &piece.url, login.is_some())
                            .map(|why| format!("request failed: {why}"))
                            .unwrap_or_else(|| format!("request failed: http status: {code}")),
                    ));
                    return;
                }
                Err(e) => {
                    send(failed(format!("request failed: {e}")));
                    return;
                }
            };
            let body = response.into_body();
            let announced = body.content_length();
            let mut reader = body.into_reader();
            // the bar is the whole bundle's when every size is known, else this file's alone
            let (base, bar) = match total {
                Some(_) => (before, total),
                None => (0, announced),
            };
            // a piece the list gave a hash for is hashed on its way in
            let mut digest = piece.sha256.as_ref().map(|_| Sha256::new());
            let strict = with.strict_size;
            let Ok(written) = stream_to_part_at(
                &mut reader,
                base,
                bar,
                &part_path,
                &|event| {
                    send(match event {
                        JobEvent::Failed(m) => failed(m),
                        other => other,
                    })
                },
                digest.as_mut(),
            ) else {
                return;
            };
            if let (Some(size), None) = (piece.size.filter(|_| strict), &piece.sha256) {
                if written != size {
                    let _ = std::fs::remove_file(&part_path);
                    send(failed(format!(
                        "{} came to {written} bytes and the configuration says {size}: it was not kept",
                        piece.name
                    )));
                    return;
                }
            }
            if let (Some(want), Some(digest)) = (&piece.sha256, digest) {
                let got: String = digest
                    .finalize()
                    .iter()
                    .map(|b| format!("{b:02x}"))
                    .collect();
                let wrong_length = piece.size.filter(|size| *size != written);
                if wrong_length.is_some() || got != *want {
                    let _ = std::fs::remove_file(&part_path);
                    send(failed(match wrong_length {
                        Some(size) => format!(
                            "{} came to {written} bytes and the list says {size}: it was not kept",
                            piece.name
                        ),
                        None => format!(
                            "{} does not match the checksum the list was made with (expected {}, got {}): it was not kept",
                            piece.name,
                            short(want),
                            short(&got)
                        ),
                    }));
                    return;
                }
            }
            if let Err(e) = std::fs::rename(&part_path, &final_path) {
                send(failed(format!(
                    "could not finalize {}: {e}",
                    final_path.display()
                )));
                return;
            }
            before += written;
            // where a file went, for an engine that keeps its files by the folder they are in: the
            // person looking for it in the window finds it nowhere else
            if let Some(place) = piece.place.as_deref() {
                send(JobEvent::Status(format!("{name} is in {place}.")));
            }
        }
        let weights_folder = download
            .primary()
            .and_then(|w| folder_of(w).ok())
            .unwrap_or_else(|| models_dir.clone());
        if let Err(e) = bundles::write(&weights_folder.join(&weights), &download) {
            send(JobEvent::Failed(format!(
                "the files are in the models folder, but the record that pairs them could not be \
                 written: {e}"
            )));
            return;
        }
        send(JobEvent::Done(weights.clone()));
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    /// Real download against Hugging Face's API (small JSON, not an actual
    /// GGUF — this exercises the streaming/rename mechanics, not the
    /// model itself), matching this repo's convention of testing against
    /// real network/process resources rather than mocks.
    #[test]
    fn downloads_and_renames_into_place() {
        let dir = std::env::temp_dir().join(format!("quickai-dl-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let mut job = spawn_download(
            "https://huggingface.co/api/models/unsloth/Qwen3-1.7B".to_string(),
            Some("test-download.json".to_string()),
            dir.clone(),
            || {},
        );

        let filename = job
            .wait(Duration::from_secs(20))
            .expect("download did not finish within timeout")
            .expect("download failed");
        let path = dir.join(&filename);
        assert!(path.is_file(), "expected {} to exist", path.display());
        assert!(std::fs::metadata(&path).unwrap().len() > 0);
        assert!(
            !dir.join(format!("{filename}.part")).exists(),
            "leftover .part file"
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A download of `me/tiny-vl-GGUF` from the fake hub: the weights, and a projector if asked.
    fn tiny(hub: &crate::testkit::fake_hub::FakeHub, with_projector: bool) -> Download {
        use quickai_core::lookup::huggingface::file_url;
        use quickai_core::lookup::present::Piece;
        let piece =
            |kind: Option<&str>, label: &str, store_as: &str, name: &str, size: u64| Piece {
                kind: kind.map(str::to_string),
                label: label.to_string(),
                url: file_url(&hub.base(), "me/tiny-vl-GGUF", "main", name),
                store_as: store_as.to_string(),
                name: name.to_string(),
                size: Some(size),
                sha256: None,
                place: None,
            };
        let mut files = vec![piece(
            None,
            "model",
            "tiny-Q4_K_M.gguf",
            "tiny-Q4_K_M.gguf",
            3000,
        )];
        if with_projector {
            files.push(piece(
                Some("mmproj"),
                "projector",
                "me--tiny-vl-GGUF--mmproj-f16.gguf",
                "mmproj-f16.gguf",
                1000,
            ));
        }
        Download {
            files,
            bundle: with_projector.then(|| "vision".to_string()),
            source: "huggingface:me/tiny-vl-GGUF".into(),
            mode: None,
        }
    }

    fn tiny_hub() -> crate::testkit::fake_hub::FakeHub {
        let hub = crate::testkit::fake_hub::FakeHub::start();
        hub.add_repo(
            "me/tiny-vl-GGUF",
            Some("image-text-to-text"),
            &["gguf"],
            &[("tiny-Q4_K_M.gguf", 3000), ("mmproj-f16.gguf", 1000)],
        );
        hub
    }

    /// What a job came to, what it said on the way and how far along it was each time it was asked.
    type Ran = (Result<String, String>, Vec<String>, Vec<(u64, Option<u64>)>);

    /// Runs a job to its end, keeping what it said on the way.
    fn run_job(job: &mut Job<String>) -> Ran {
        let mut statuses = Vec::new();
        let mut progress = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(20);
        loop {
            let outcome = job.poll();
            statuses.extend(job.take_statuses());
            progress.push((job.downloaded, job.total));
            if let Some(outcome) = outcome {
                return (outcome, statuses, progress);
            }
            assert!(
                std::time::Instant::now() < deadline,
                "the download did not end"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn p3_b12_a_bundle_is_one_job_that_fetches_the_weights_and_what_goes_with_them_and_records_the_pair(
    ) {
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("b12");
        let mut job = spawn_files(tiny(&hub, true), dir.join("models"), || {});
        assert_eq!(job.verb, "Downloading");
        assert_eq!(job.label, "tiny-Q4_K_M.gguf and 1 more file");
        let (outcome, statuses, progress) = run_job(&mut job);
        assert_eq!(
            outcome.unwrap(),
            "tiny-Q4_K_M.gguf",
            "the weights are what is selected afterwards"
        );
        // both files are in place with all their bytes, and nothing half-written is left
        let models = dir.join("models");
        assert_eq!(
            std::fs::metadata(models.join("tiny-Q4_K_M.gguf"))
                .unwrap()
                .len(),
            3000
        );
        assert_eq!(
            std::fs::metadata(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf"))
                .unwrap()
                .len(),
            1000
        );
        assert!(std::fs::read_to_string(models.join("tiny-Q4_K_M.gguf"))
            .unwrap()
            .starts_with("me/tiny-vl-GGUF:tiny-Q4_K_M.gguf|"));
        let mut left: Vec<String> = std::fs::read_dir(&models)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "me--tiny-vl-GGUF--mmproj-f16.gguf",
                "tiny-Q4_K_M.gguf",
                "tiny-Q4_K_M.gguf.bundle.json"
            ]
        );
        // the weights first, then the projector, each once
        assert_eq!(
            hub.downloads(),
            [
                "me/tiny-vl-GGUF:tiny-Q4_K_M.gguf",
                "me/tiny-vl-GGUF:mmproj-f16.gguf"
            ]
        );
        // the log is told of each part
        assert_eq!(
            statuses,
            [
                "Downloading tiny-Q4_K_M.gguf (1 of 2)",
                "Downloading mmproj-f16.gguf (2 of 2)"
            ]
        );
        // the bar is the bundle's: bytes of both against the sum, and it never goes back
        let counted: Vec<u64> = progress
            .iter()
            .filter(|(_, t)| *t == Some(4000))
            .map(|(d, _)| *d)
            .collect();
        assert!(counted.windows(2).all(|w| w[0] <= w[1]), "{counted:?}");
        assert_eq!(counted.last(), Some(&4000));
        // and the record pairs them
        let companions = crate::bundles::companions(&models.join("tiny-Q4_K_M.gguf"));
        assert_eq!(companions.len(), 1);
        assert_eq!(
            (companions[0].kind.as_str(), companions[0].name.as_str()),
            ("mmproj", "mmproj-f16.gguf")
        );
        assert!(companions[0].exists());
        // the weights alone are what they have always been: no record
        let alone_dir = crate::testkit::scratch("b12-alone");
        let mut job = spawn_files(tiny(&hub, false), alone_dir.join("m"), || {});
        assert_eq!(job.label, "tiny-Q4_K_M.gguf");
        let (outcome, statuses, _) = run_job(&mut job);
        assert_eq!(outcome.unwrap(), "tiny-Q4_K_M.gguf");
        assert!(
            statuses.is_empty(),
            "one file says nothing of parts: {statuses:?}"
        );
        assert!(!alone_dir.join("m/tiny-Q4_K_M.gguf.bundle.json").exists());
    }

    /// The download `tiny` makes, with the hash the fake's bytes really have on each piece.
    fn tiny_checked(hub: &crate::testkit::fake_hub::FakeHub, with_projector: bool) -> Download {
        use crate::testkit::fake_hub::{content, sha256_hex};
        let mut download = tiny(hub, with_projector);
        for piece in &mut download.files {
            let bytes = content("me/tiny-vl-GGUF", &piece.name, piece.size.unwrap() as usize);
            piece.sha256 = Some(sha256_hex(&bytes));
        }
        download
    }

    #[test]
    fn p3_j13_a_piece_with_a_hash_is_checked_while_it_arrives_and_a_wrong_one_is_not_kept() {
        // the right bytes: kept, with the record and nothing left over
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("j13");
        let models = dir.join("models");
        let (outcome, _, _) = run_job(&mut spawn_files(
            tiny_checked(&hub, true),
            models.clone(),
            || {},
        ));
        assert_eq!(outcome.unwrap(), "tiny-Q4_K_M.gguf");
        assert!(models.join("tiny-Q4_K_M.gguf").is_file());
        assert!(models.join("tiny-Q4_K_M.gguf.bundle.json").is_file());

        // the wrong hash on the weights: nothing is kept, not even a half, and the message says why
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("j13-weights");
        let models = dir.join("models");
        let mut download = tiny_checked(&hub, false);
        download.files[0].sha256 = Some("0".repeat(64));
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        let message = outcome.unwrap_err();
        assert!(
            message.contains("tiny-Q4_K_M.gguf does not match the checksum the list was made with")
                && message.contains("expected 000000000000…")
                && message.contains("it was not kept"),
            "{message}"
        );
        let left: Vec<String> = std::fs::read_dir(&models)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(
            left.is_empty(),
            "nothing stays of a file that is not the one: {left:?}"
        );

        // the right hash and a size that is not the one: the same
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("j13-size");
        let models = dir.join("models");
        let mut download = tiny_checked(&hub, false);
        // the fake serves as many bytes as the repository lists; the list the hash came from says another
        download.files[0].size = Some(3001);
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        let message = outcome.unwrap_err();
        assert!(
            message.contains("came to 3000 bytes and the list says 3001"),
            "{message}"
        );
        assert!(std::fs::read_dir(&models).unwrap().next().is_none());

        // a piece with no hash is as before: a size that is not the one is not looked at
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("j13-plain");
        let models = dir.join("models");
        let mut download = tiny(&hub, false);
        download.files[0].size = Some(3001);
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        assert_eq!(outcome.unwrap(), "tiny-Q4_K_M.gguf");

        // the projector is wrong: the weights stay and run alone, the projector is not kept and there
        // is no record that says the two belong together
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("j13-projector");
        let models = dir.join("models");
        let mut download = tiny_checked(&hub, true);
        download.files[1].sha256 = Some("f".repeat(64));
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        let message = outcome.unwrap_err();
        assert!(
            message.contains("could not fetch mmproj-f16.gguf")
                && message.contains("does not match the checksum")
                && message.contains("tiny-Q4_K_M.gguf was fetched and runs without it"),
            "{message}"
        );
        assert!(models.join("tiny-Q4_K_M.gguf").is_file());
        assert!(!models.join("me--tiny-vl-GGUF--mmproj-f16.gguf").exists());
        assert!(!models
            .join("me--tiny-vl-GGUF--mmproj-f16.gguf.part")
            .exists());
        assert!(!models.join("tiny-Q4_K_M.gguf.bundle.json").exists());
    }

    #[test]
    fn p3_b13_a_file_that_is_already_there_at_its_size_is_not_fetched_again() {
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("b13");
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        // the projector of another quantization of the repository, as it was fetched then
        std::fs::write(
            models.join("me--tiny-vl-GGUF--mmproj-f16.gguf"),
            vec![b'p'; 1000],
        )
        .unwrap();
        let (outcome, statuses, progress) =
            run_job(&mut spawn_files(tiny(&hub, true), models.clone(), || {}));
        assert_eq!(outcome.unwrap(), "tiny-Q4_K_M.gguf");
        assert_eq!(
            hub.downloads(),
            ["me/tiny-vl-GGUF:tiny-Q4_K_M.gguf"],
            "only the weights were asked for"
        );
        assert!(
            statuses.contains(
                &"me--tiny-vl-GGUF--mmproj-f16.gguf is already in the models folder.".to_string()
            ),
            "{statuses:?}"
        );
        assert_eq!(
            progress.last().unwrap().0,
            4000,
            "the bar still counts the file that was there"
        );
        assert_eq!(
            std::fs::read(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf")).unwrap(),
            vec![b'p'; 1000],
            "untouched"
        );
        assert!(models.join("tiny-Q4_K_M.gguf.bundle.json").exists());

        // a file of the same name and another size is another file: it is fetched
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("b13-size");
        let models = dir.join("models");
        std::fs::create_dir_all(&models).unwrap();
        std::fs::write(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf"), b"short").unwrap();
        let (outcome, _, _) = run_job(&mut spawn_files(tiny(&hub, true), models.clone(), || {}));
        assert!(outcome.is_ok());
        assert_eq!(hub.downloads().len(), 2);
        assert_eq!(
            std::fs::metadata(models.join("me--tiny-vl-GGUF--mmproj-f16.gguf"))
                .unwrap()
                .len(),
            1000
        );
    }

    #[test]
    fn p3_b14_a_file_after_the_weights_that_fails_leaves_the_weights_and_says_what_was_not_fetched()
    {
        use crate::testkit::mini_http::{MiniServer, Request};
        // the site has the weights and refuses the projector
        let hub = tiny_hub();
        let mut download = tiny(&hub, true);
        download.files[1].url = format!(
            "{}/me/tiny-vl-GGUF/resolve/main/not-there.gguf",
            "http://127.0.0.1:1"
        );
        let dir = crate::testkit::scratch("b14");
        let models = dir.join("models");
        let (outcome, statuses, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        let message = outcome.unwrap_err();
        assert!(
            message.contains("could not fetch mmproj-f16.gguf")
                && message.contains("tiny-Q4_K_M.gguf was fetched and runs without it"),
            "{message}"
        );
        assert!(statuses.contains(&"Downloading mmproj-f16.gguf (2 of 2)".to_string()));
        assert!(
            models.join("tiny-Q4_K_M.gguf").is_file(),
            "the weights stay: they run alone"
        );
        assert!(
            !models.join("tiny-Q4_K_M.gguf.bundle.json").exists(),
            "and nothing says they have a pair"
        );
        assert!(!models
            .join("me--tiny-vl-GGUF--mmproj-f16.gguf.part")
            .exists());

        // the weights fail: nothing else is asked for, and the message is the plain one
        let asked = std::sync::Arc::new(std::sync::Mutex::new(Vec::<String>::new()));
        let log = asked.clone();
        let broken = MiniServer::start(None, move |req: &Request, resp| {
            log.lock().unwrap().push(req.path.clone());
            resp.status(404);
            resp.send(b"no");
        })
        .unwrap();
        let hub = tiny_hub();
        let mut download = tiny(&hub, true);
        for piece in &mut download.files {
            piece.url = format!("http://127.0.0.1:{}/x/{}", broken.port(), piece.name);
        }
        let dir = crate::testkit::scratch("b14-weights");
        let (outcome, _, _) = run_job(&mut spawn_files(download, dir.join("m"), || {}));
        let message = outcome.unwrap_err();
        assert!(message.starts_with("request failed"), "{message}");
        assert_eq!(
            asked.lock().unwrap().len(),
            1,
            "the projector was not asked for"
        );
        assert!(!dir.join("m/tiny-Q4_K_M.gguf").exists());
    }

    #[test]
    fn p3_b15_a_name_that_climbs_out_of_the_folder_cannot() {
        let hub = tiny_hub();
        let dir = crate::testkit::scratch("b15");
        let models = dir.join("models");
        let mut download = tiny(&hub, false);
        download.files[0].store_as = "../../escaped.gguf".into();
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        assert_eq!(outcome.unwrap(), "escaped.gguf");
        assert!(models.join("escaped.gguf").is_file());
        assert!(!dir.join("escaped.gguf").exists());
        assert!(!dir.parent().unwrap().join("escaped.gguf").exists());
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

    /// A local path (not a URL) should be imported by filesystem link/copy
    /// rather than an HTTP request, landing in models_dir under its own
    /// filename with identical contents. Source and dest here share a
    /// filesystem (both under the OS temp dir), so this exercises the
    /// symlink fast path specifically — the source file must stay in place
    /// under its own path, untouched, with only a link appearing in
    /// models_dir.
    #[test]
    fn imports_local_file_by_path() {
        let base = std::env::temp_dir().join(format!("quickai-import-test-{}", std::process::id()));
        let source_dir = base.join("source");
        let dest_dir = base.join("dest");
        std::fs::create_dir_all(&source_dir).unwrap();

        let source_path = source_dir.join("my-model.gguf");
        std::fs::write(&source_path, b"fake gguf contents").unwrap();

        let mut job = spawn_download(
            source_path.display().to_string(),
            None,
            dest_dir.clone(),
            || {},
        );
        assert_eq!(job.verb, "Importing");

        let filename = job
            .wait(Duration::from_secs(10))
            .expect("import did not finish within timeout")
            .expect("import failed");
        assert_eq!(filename, "my-model.gguf");
        let imported_path = dest_dir.join(&filename);
        assert_eq!(
            std::fs::read(&imported_path).unwrap(),
            b"fake gguf contents"
        );
        // Unix can always create a symlink without special privilege, so
        // this is guaranteed there; Windows needs Developer Mode/admin for
        // symlinks and falls back to a hard link instead, which this test
        // doesn't have a portable std-only way to distinguish from a copy.
        #[cfg(unix)]
        assert!(
            std::fs::symlink_metadata(&imported_path)
                .unwrap()
                .file_type()
                .is_symlink(),
            "expected a symlink into models_dir, not a copy"
        );
        assert!(
            source_path.is_file(),
            "source file should stay in place, untouched"
        );
        assert!(
            !dest_dir.join(format!("{filename}.part")).exists(),
            "leftover .part file"
        );

        let _ = std::fs::remove_dir_all(&base);
    }

    /// Unix can always create a symlink as an unprivileged user, so this
    /// checks `try_symlink` directly rather than only indirectly through
    /// `spawn_download` — including that reading through the link
    /// transparently returns the source's contents.
    #[cfg(unix)]
    #[test]
    fn try_symlink_creates_a_working_link() {
        let base =
            std::env::temp_dir().join(format!("quickai-symlink-test-{}", std::process::id()));
        std::fs::create_dir_all(&base).unwrap();
        let source = base.join("source.gguf");
        std::fs::write(&source, b"abc").unwrap();
        let link = base.join("link.gguf");

        try_symlink(&source, &link).expect("symlink should succeed on Unix");
        assert!(std::fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(std::fs::read(&link).unwrap(), b"abc");

        let _ = std::fs::remove_dir_all(&base);
    }

    // ---- a file of a kind of its own, fetched by itself (work package 4.13, group Q) ----------------------

    /// The one companion file `VAE/tiny_vae.safetensors` of the fake's `me/tiny-video-GGUF`, as a button of
    /// a row of its own fetches it: no weights, a folder of its kind, the name it has, and its hash.
    fn alone_vae(hub: &crate::testkit::fake_hub::FakeHub) -> Download {
        use crate::testkit::fake_hub::{content, sha256_hex};
        let bytes = content("me/tiny-video-GGUF", "VAE/tiny_vae.safetensors", 2000);
        Download {
            files: vec![Piece {
                kind: Some("vae".into()),
                label: "VAE".into(),
                url: quickai_core::lookup::huggingface::file_url(
                    &hub.base(),
                    "me/tiny-video-GGUF",
                    "main",
                    "VAE/tiny_vae.safetensors",
                ),
                store_as: "tiny_vae.safetensors".into(),
                name: "tiny_vae.safetensors".into(),
                size: Some(2000),
                sha256: Some(sha256_hex(&bytes)),
                place: Some("vae".into()),
            }],
            bundle: None,
            source: "huggingface:me/tiny-video-GGUF".into(),
            mode: None,
        }
    }

    fn video_hub() -> crate::testkit::fake_hub::FakeHub {
        let hub = crate::testkit::fake_hub::FakeHub::start();
        hub.add_repo(
            "me/tiny-video-GGUF",
            Some("text-to-video"),
            &["gguf"],
            &[
                ("tiny-video-Q8_0.gguf", 5000),
                ("VAE/tiny_vae.safetensors", 2000),
            ],
        );
        hub
    }

    #[test]
    fn p3_q13_a_file_of_a_kind_of_its_own_is_a_download_named_after_it_kept_in_its_folder_with_no_record(
    ) {
        let hub = video_hub();
        let dir = crate::testkit::scratch("q13");
        let models = dir.join("models");
        let download = alone_vae(&hub);
        assert!(download.weights().is_none());
        assert_eq!(download.primary().unwrap().store_as, "tiny_vae.safetensors");
        let mut job = spawn_files(download, models.clone(), || {});
        assert_eq!(
            job.label, "tiny_vae.safetensors",
            "named after its one file, not after nothing"
        );
        let (outcome, statuses, progress) = run_job(&mut job);
        assert_eq!(outcome.unwrap(), "tiny_vae.safetensors");
        assert_eq!(
            std::fs::metadata(models.join("vae/tiny_vae.safetensors"))
                .unwrap()
                .len(),
            2000
        );
        assert!(
            statuses.contains(&"tiny_vae.safetensors is in vae.".to_string()),
            "{statuses:?}"
        );
        assert_eq!(progress.last().unwrap(), &(2000, Some(2000)));
        // nothing pairs it with anything: no record, no part, nothing in the models folder itself
        let left: Vec<String> = std::fs::read_dir(&models)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(left, ["vae"]);
        let inside: Vec<String> = std::fs::read_dir(models.join("vae"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(inside, ["tiny_vae.safetensors"]);

        // there already, at its size: it is not fetched again
        let asked = hub.downloads().len();
        let (outcome, statuses, _) =
            run_job(&mut spawn_files(alone_vae(&hub), models.clone(), || {}));
        assert_eq!(outcome.unwrap(), "tiny_vae.safetensors");
        assert_eq!(hub.downloads().len(), asked);
        assert!(
            statuses
                .iter()
                .any(|s| s.contains("already in the models folder")),
            "{statuses:?}"
        );

        // a file that is not the one the hub states is not kept, not even as a part
        let hub = video_hub();
        let dir = crate::testkit::scratch("q13-wrong");
        let models = dir.join("models");
        let mut download = alone_vae(&hub);
        download.files[0].sha256 = Some("f".repeat(64));
        let (outcome, _, _) = run_job(&mut spawn_files(download, models.clone(), || {}));
        let message = outcome.unwrap_err();
        assert!(
            message.contains("does not match the checksum") && message.contains("it was not kept"),
            "{message}"
        );
        assert!(
            !message.contains("was fetched and runs without it"),
            "it is the one file: {message}"
        );
        let inside: Vec<String> = std::fs::read_dir(models.join("vae"))
            .map(|r| {
                r.map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
                    .collect()
            })
            .unwrap_or_default();
        assert!(inside.is_empty(), "{inside:?}");

        // a folder that is not a plain name is no folder
        let hub = video_hub();
        let dir = crate::testkit::scratch("q13-climb");
        let mut download = alone_vae(&hub);
        download.files[0].place = Some("../escape".into());
        let (outcome, _, _) = run_job(&mut spawn_files(download, dir.join("models"), || {}));
        assert!(outcome.unwrap_err().contains("is not the name of a folder"));
        assert!(!dir.join("escape").exists());
    }
}
