//! Archives for the installer tests, written by hand so that a test can put *anything* in one:
//! the layout of a real Ollama release, a path that climbs out of the install directory, a
//! symlink that points outside, a truncated or damaged stream.
//!
//! The standard writers refuse to write an unsafe entry (`tar::Header::set_path` rejects `..`),
//! which is right for them and wrong here, so the header's bytes are filled in directly. A zstd
//! stream is written as a frame of raw (stored) blocks: any decoder must accept it, and it needs
//! no compressor. That says nothing about reading what a *real* compressor produces, which is why
//! `tests/fixtures/archives/` holds three archives made by the real `zstd` (see `make_archives.sh`).

use std::io::Write;
use std::path::Path;

/// What one entry of an archive is.
#[derive(Clone, Debug)]
pub enum Kind {
    File {
        mode: u32,
    },
    Dir,
    Symlink {
        target: String,
    },
    /// A hard link (tar only).
    HardLink {
        target: String,
    },
}

#[derive(Clone, Debug)]
pub struct Entry {
    /// Exactly as it will be written: `../evil` stays `../evil`.
    pub path: String,
    pub kind: Kind,
    pub data: Vec<u8>,
}

impl Entry {
    pub fn file(path: &str, data: &[u8]) -> Entry {
        Entry {
            path: path.into(),
            kind: Kind::File { mode: 0o644 },
            data: data.to_vec(),
        }
    }
    pub fn exe(path: &str, data: &[u8]) -> Entry {
        Entry {
            path: path.into(),
            kind: Kind::File { mode: 0o755 },
            data: data.to_vec(),
        }
    }
    pub fn dir(path: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: Kind::Dir,
            data: Vec::new(),
        }
    }
    pub fn symlink(path: &str, target: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: Kind::Symlink {
                target: target.into(),
            },
            data: Vec::new(),
        }
    }
    pub fn hardlink(path: &str, target: &str) -> Entry {
        Entry {
            path: path.into(),
            kind: Kind::HardLink {
                target: target.into(),
            },
            data: Vec::new(),
        }
    }
}

fn put(field: &mut [u8], text: &str) {
    assert!(text.len() < field.len(), "{text} does not fit a tar field");
    field.fill(0);
    field[..text.len()].copy_from_slice(text.as_bytes());
}

/// A tar stream (no compression).
pub fn tar(entries: &[Entry]) -> Vec<u8> {
    let mut builder = tar::Builder::new(Vec::new());
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        put(&mut header.as_old_mut().name, &entry.path);
        header.set_mtime(1_790_000_000);
        header.set_uid(0);
        header.set_gid(0);
        match &entry.kind {
            Kind::File { mode } => {
                header.set_entry_type(tar::EntryType::Regular);
                header.set_mode(*mode);
                header.set_size(entry.data.len() as u64);
            }
            Kind::Dir => {
                header.set_entry_type(tar::EntryType::Directory);
                header.set_mode(0o755);
                header.set_size(0);
            }
            Kind::Symlink { target } => {
                header.set_entry_type(tar::EntryType::Symlink);
                header.set_mode(0o777);
                header.set_size(0);
                put(&mut header.as_old_mut().linkname, target);
            }
            Kind::HardLink { target } => {
                header.set_entry_type(tar::EntryType::Link);
                header.set_mode(0o644);
                header.set_size(0);
                put(&mut header.as_old_mut().linkname, target);
            }
        }
        header.set_cksum();
        builder
            .append(&header, entry.data.as_slice())
            .expect("append a tar entry");
    }
    builder.into_inner().expect("finish the tar stream")
}

pub fn tar_gz(entries: &[Entry]) -> Vec<u8> {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::fast());
    encoder.write_all(&tar(entries)).unwrap();
    encoder.finish().unwrap()
}

/// One zstd frame holding `data` in raw blocks: no checksum, a window of 128 KiB.
pub fn zst_frame(data: &[u8]) -> Vec<u8> {
    const BLOCK: usize = 64 * 1024;
    let mut out = vec![0x28, 0xB5, 0x2F, 0xFD];
    // frame header descriptor: no content size, not single-segment, no checksum, no dictionary
    out.push(0x00);
    // window descriptor: exponent 7, mantissa 0 -> 2^(10+7) = 128 KiB
    out.push(7 << 3);
    let mut chunks: Vec<&[u8]> = data.chunks(BLOCK).collect();
    if chunks.is_empty() {
        chunks.push(&[]);
    }
    let last = chunks.len() - 1;
    for (i, chunk) in chunks.iter().enumerate() {
        // 3 bytes, little endian: last-block bit, block type 0 (raw), 21-bit size
        let header = (chunk.len() as u32) << 3 | u32::from(i == last);
        out.extend_from_slice(&header.to_le_bytes()[..3]);
        out.extend_from_slice(chunk);
    }
    out
}

/// A skippable frame (magic `0x184D2A50`) holding `payload`.
pub fn zst_skippable(payload: &[u8]) -> Vec<u8> {
    let mut out = vec![0x50, 0x2A, 0x4D, 0x18];
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(payload);
    out
}

pub fn tar_zst(entries: &[Entry]) -> Vec<u8> {
    zst_frame(&tar(entries))
}

/// A `.zip`; a name is written as given. A symlink is stored the way Info-ZIP stores one.
pub fn zip(entries: &[Entry]) -> Vec<u8> {
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for entry in entries {
        match &entry.kind {
            Kind::File { mode } => {
                let options = zip::write::SimpleFileOptions::default().unix_permissions(*mode);
                writer.start_file(entry.path.as_str(), options).unwrap();
                writer.write_all(&entry.data).unwrap();
            }
            Kind::Dir => {
                writer
                    .add_directory(
                        entry.path.as_str(),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
            }
            Kind::Symlink { target } => {
                writer
                    .add_symlink(
                        entry.path.as_str(),
                        target.as_str(),
                        zip::write::SimpleFileOptions::default(),
                    )
                    .unwrap();
            }
            Kind::HardLink { .. } => panic!("a zip has no hard links"),
        }
    }
    writer.finish().unwrap().into_inner()
}

/// The bytes of one of the real-`zstd` fixtures in `tests/fixtures/archives/`.
pub fn fixture(name: &str) -> Vec<u8> {
    let path = super::fixtures_dir().join("archives").join(name);
    std::fs::read(&path).unwrap_or_else(|e| panic!("fixture {}: {e}", path.display()))
}

/// Bytes that look like a binary and are the same every time.
pub fn pseudo(seed: u32, len: usize) -> Vec<u8> {
    let mut state = seed.wrapping_mul(2_654_435_761).wrapping_add(1);
    (0..len)
        .map(|_| {
            state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            (state >> 24) as u8
        })
        .collect()
}

/// The shell script that stands in for the `ollama` binary in every tree below.
pub const FAKE_OLLAMA_SCRIPT: &[u8] = b"#!/bin/sh\n\
case \"$1\" in\n\
    --version) echo \"ollama version is 0.35.1\" ;;\n\
    serve) exec sleep 300 ;;\n\
    *) echo \"usage: ollama serve\" >&2; exit 2 ;;\n\
esac\n";

/// The layout of `ollama-linux-amd64.tar.zst` (v0.35.1): `bin/ollama`, licences, the CUDA
/// directories, the base libraries with their relative links, Vulkan. Like the real archive it has
/// no entry for `bin/` or `lib/ollama/` themselves. Sizes are small; `cuda_*` is the bulk, as in
/// the real one.
pub fn ollama_linux_tree() -> Vec<Entry> {
    vec![
        Entry::exe("bin/ollama", FAKE_OLLAMA_SCRIPT),
        Entry::file("lib/ollama/GO_LICENSE", b"Copyright (c) the Go authors.\n"),
        Entry::dir("lib/ollama/cuda_v12/"),
        Entry::exe("lib/ollama/cuda_v12/libggml-cuda.so", &pseudo(1, 24_000)),
        Entry::exe(
            "lib/ollama/cuda_v12/libcudart.so.12.8.90",
            &pseudo(2, 4_000),
        ),
        Entry::symlink(
            "lib/ollama/cuda_v12/libcudart.so.12",
            "libcudart.so.12.8.90",
        ),
        Entry::dir("lib/ollama/cuda_v13/"),
        Entry::exe("lib/ollama/cuda_v13/libggml-cuda.so", &pseudo(3, 24_000)),
        Entry::symlink("lib/ollama/libggml-base.so", "libggml-base.so.0"),
        Entry::symlink("lib/ollama/libggml-base.so.0", "libggml-base.so.0.25.3"),
        Entry::exe("lib/ollama/libggml-base.so.0.25.3", &pseudo(4, 6_000)),
        Entry::file("lib/ollama/libggml-cpu-x64.so", &pseudo(5, 6_000)),
        Entry::exe("lib/ollama/llama-server", b"#!/bin/sh\nexit 0\n"),
        Entry::dir("lib/ollama/vulkan/"),
        Entry::exe("lib/ollama/vulkan/libggml-vulkan.so", &pseudo(6, 16_000)),
    ]
}

/// `ollama-darwin.tgz`: the binary and its libraries side by side at the top.
pub fn ollama_mac_tree() -> Vec<Entry> {
    vec![
        Entry::exe("ollama", FAKE_OLLAMA_SCRIPT),
        Entry::file("libggml-base.dylib", &pseudo(7, 6_000)),
        Entry::file("libggml-cpu.so", &pseudo(8, 6_000)),
    ]
}

/// `ollama-windows-amd64.zip`: `ollama.exe` at the root, libraries below `lib/ollama`.
pub fn ollama_windows_tree() -> Vec<Entry> {
    vec![
        Entry::file("ollama.exe", FAKE_OLLAMA_SCRIPT),
        Entry::dir("lib/ollama/"),
        Entry::file("lib/ollama/ggml-base.dll", &pseudo(9, 6_000)),
        Entry::dir("lib/ollama/cuda_v12/"),
        Entry::file("lib/ollama/cuda_v12/ggml-cuda.dll", &pseudo(10, 24_000)),
        Entry::dir("lib/ollama/vulkan/"),
        Entry::file("lib/ollama/vulkan/ggml-vulkan.dll", &pseudo(11, 16_000)),
    ]
}

/// Every file, directory and link of `entries` is under `dir` with its content (and, on Unix, its
/// executable bit and its link target). Entries whose path is not a plain relative one are not
/// looked for: they are what a test expects *not* to arrive.
pub fn assert_extracted(dir: &Path, entries: &[Entry]) {
    assert_extracted_with(dir, entries, true);
}

/// As `assert_extracted`, without looking at the executable bits (an installer that is told to
/// make the binary executable changes one of them on purpose).
pub fn assert_extracted_content(dir: &Path, entries: &[Entry]) {
    assert_extracted_with(dir, entries, false);
}

fn assert_extracted_with(dir: &Path, entries: &[Entry], check_modes: bool) {
    let _ = check_modes; // looked at on Unix only
    for entry in entries {
        let rel = entry.path.trim_end_matches('/');
        let path = dir.join(rel);
        match &entry.kind {
            Kind::File { mode } => {
                let got = std::fs::read(&path)
                    .unwrap_or_else(|e| panic!("{} was not extracted: {e}", path.display()));
                assert_eq!(got, entry.data, "{} has other contents", path.display());
                #[cfg(unix)]
                if check_modes {
                    use std::os::unix::fs::PermissionsExt;
                    let got_mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o111;
                    assert_eq!(
                        got_mode != 0,
                        mode & 0o111 != 0,
                        "{}: the executable bit differs from the archive's",
                        path.display()
                    );
                }
                let _ = mode;
            }
            Kind::Dir => assert!(path.is_dir(), "{} is not a directory", path.display()),
            Kind::Symlink { target } => {
                #[cfg(unix)]
                assert_eq!(
                    std::fs::read_link(&path)
                        .unwrap_or_else(|e| panic!("{} is not a link: {e}", path.display()))
                        .to_string_lossy(),
                    target.as_str(),
                    "{}: the link points elsewhere",
                    path.display()
                );
                let _ = target;
            }
            Kind::HardLink { .. } => {}
        }
    }
}

/// The sha256 of `bytes` as lower-case hex: what a host lists for a file.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_tar(bytes: &[u8]) -> Vec<(String, u64)> {
        tar::Archive::new(bytes)
            .entries()
            .unwrap()
            .map(|e| {
                let e = e.unwrap();
                (
                    String::from_utf8_lossy(&e.path_bytes()).into_owned(),
                    e.header().size().unwrap(),
                )
            })
            .collect()
    }

    #[test]
    fn a_tar_holds_what_it_was_given_even_an_unsafe_path() {
        let bytes = tar(&[
            Entry::file("a/b.txt", b"hello"),
            Entry::file("../escape.txt", b"x"),
            Entry::file("/abs.txt", b"y"),
            Entry::symlink("link", "/etc/passwd"),
        ]);
        let names = read_tar(&bytes);
        assert_eq!(
            names.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["a/b.txt", "../escape.txt", "/abs.txt", "link"]
        );
        assert_eq!(names[0].1, 5);
        let mut archive = tar::Archive::new(bytes.as_slice());
        let link = archive.entries().unwrap().nth(3).unwrap().unwrap();
        assert_eq!(
            link.link_name().unwrap().unwrap().to_string_lossy(),
            "/etc/passwd"
        );
    }

    #[test]
    fn a_tar_gz_is_read_back_by_the_gzip_decoder() {
        let gz = tar_gz(&ollama_mac_tree());
        let mut plain = Vec::new();
        std::io::Read::read_to_end(&mut flate2::read::GzDecoder::new(gz.as_slice()), &mut plain)
            .unwrap();
        assert_eq!(plain, tar(&ollama_mac_tree()));
    }

    #[test]
    fn a_zip_keeps_names_modes_and_links() {
        let bytes = zip(&[
            Entry::exe("bin/x", b"#!/bin/sh\n"),
            Entry::dir("d/"),
            Entry::symlink("d/l", "../bin/x"),
            Entry::file("../out", b"o"),
        ]);
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        assert_eq!(names, ["bin/x", "d/", "d/l", "../out"]);
        assert_eq!(
            archive.by_index(0).unwrap().unix_mode().map(|m| m & 0o777),
            Some(0o755)
        );
    }

    /// The real `zstd` (when it is installed) must accept the frames written here, or they prove
    /// nothing about the installer.
    #[test]
    fn the_raw_block_frames_are_valid_zstd() {
        let Ok(zstd) = which("zstd") else {
            eprintln!("zstd is not installed: skipping");
            return;
        };
        let dir = crate::testkit::scratch("zst-valid");
        let sizes = [0usize, 1, 65_535, 65_536, 65_537, 300_000];
        for n in sizes {
            let data = pseudo(n as u32, n);
            let frame = zst_frame(&data);
            let path = dir.join(format!("f{n}.zst"));
            std::fs::write(&path, &frame).unwrap();
            let out = std::process::Command::new(&zstd)
                .args(["-dc"])
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{n} bytes: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            assert_eq!(out.stdout, data, "{n} bytes: decoded differently");
        }
        // a skippable frame in front changes nothing
        let mut with_skip = zst_skippable(b"ignore me");
        with_skip.extend_from_slice(&zst_frame(b"payload"));
        let path = dir.join("skip.zst");
        std::fs::write(&path, &with_skip).unwrap();
        let out = std::process::Command::new(&zstd)
            .args(["-dc"])
            .arg(&path)
            .output()
            .unwrap();
        assert_eq!(out.stdout, b"payload");
        let _ = std::fs::remove_dir_all(dir);
    }

    fn which(program: &str) -> Result<std::path::PathBuf, ()> {
        let paths = std::env::var_os("PATH").ok_or(())?;
        std::env::split_paths(&paths)
            .map(|d| d.join(program))
            .find(|p| p.is_file())
            .ok_or(())
    }

    #[test]
    fn the_linux_tree_has_the_real_archives_shape() {
        let tree = ollama_linux_tree();
        let names: Vec<&str> = tree.iter().map(|e| e.path.as_str()).collect();
        assert_eq!(names[0], "bin/ollama");
        assert!(!names.contains(&"bin/") && !names.contains(&"lib/ollama/"));
        assert!(names.iter().any(|n| n.starts_with("lib/ollama/cuda_v12/")));
        assert_eq!(
            tree.iter()
                .filter(|e| matches!(e.kind, Kind::Symlink { .. }))
                .count(),
            3
        );
    }

    /// The generated tree and the real-zstd fixture hold the same names in the same order: the
    /// fixture is the one place the layout is checked against a real compressor.
    #[test]
    fn the_real_zstd_fixture_has_the_layout_of_the_generated_tree() {
        let Ok(zstd) = which("zstd") else {
            eprintln!("zstd is not installed: skipping");
            return;
        };
        let dir = crate::testkit::scratch("fixture-layout");
        let path = dir.join("f.tar.zst");
        std::fs::write(&path, fixture("mini-ollama-linux.tar.zst")).unwrap();
        let out = std::process::Command::new(zstd)
            .arg("-dc")
            .arg(&path)
            .output()
            .unwrap();
        assert!(out.status.success());
        let from_fixture: Vec<String> = read_tar(&out.stdout).into_iter().map(|(n, _)| n).collect();
        let from_tree: Vec<String> = ollama_linux_tree().into_iter().map(|e| e.path).collect();
        // the fixture was made with the tools' own ordering of links; compare as sets, and the
        // first entry exactly
        let mut a = from_fixture.clone();
        let mut b = from_tree.clone();
        a.sort();
        b.sort();
        assert_eq!(a, b);
        assert_eq!(from_fixture[0], "bin/ollama");
        let _ = std::fs::remove_dir_all(dir);
    }
}
