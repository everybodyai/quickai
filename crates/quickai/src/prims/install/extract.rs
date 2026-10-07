//! Unpacking an archive into a staging directory, entry by entry, with the filter of decision
//! C27 of the plan: a path must be relative and stay inside the directory, a link must be
//! relative and stay inside it, and an entry that is not a file, a directory or such a link (a
//! hard link, a device) is refused. **Any unsafe entry refuses the whole install**, where the
//! `tar` crate on its own skips an entry that climbs out and goes on, so a hostile archive would
//! be installed as if it were whole.
//!
//! The checks are QuickAI's own. Every entry is looked at before anything of it is written, the
//! parent directory it would be written into is resolved on disk (a link made earlier by the
//! archive may point somewhere unexpected, and `a -> .` followed by `a/l -> ../z` is how a link
//! that looks inside is made to point outside), and files are written by this module rather than
//! by `tar::Entry::unpack`, which would follow what the checks have already refused.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Component, Path, PathBuf};

use super::zstd::ZstdReader;

/// An archive that unpacks to more than this is refused: the largest real one is about 2 GB, and a
/// small file that expands without limit would otherwise fill the disk.
const MAX_UNPACKED_BYTES: u64 = 16 * 1024 * 1024 * 1024;
const MAX_ENTRIES: u64 = 200_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    TarGz,
    TarZst,
}

impl ArchiveKind {
    pub fn from_url(url: &str) -> Option<Self> {
        // a query string or fragment is not part of the file's name
        let name = url
            .split(['?', '#'])
            .next()
            .unwrap_or(url)
            .to_ascii_lowercase();
        if name.ends_with(".tar.zst") {
            Some(ArchiveKind::TarZst)
        } else if name.ends_with(".tar.gz") || name.ends_with(".tgz") {
            Some(ArchiveKind::TarGz)
        } else if name.ends_with(".zip") {
            Some(ArchiveKind::Zip)
        } else {
            None
        }
    }

    pub fn extension(&self) -> &'static str {
        match self {
            ArchiveKind::Zip => "zip",
            ArchiveKind::TarGz => "tar.gz",
            ArchiveKind::TarZst => "tar.zst",
        }
    }

    /// The sentence for a file this installer cannot read.
    pub const EXPECTED: &'static str = "expected a .zip, .tar.gz, .tgz or .tar.zst";
}

/// Why unpacking stopped.
#[derive(Debug)]
enum Refusal {
    /// An entry the install must not contain: the whole archive is refused.
    Unsafe(String),
    /// The archive could not be read, or the disk could not be written.
    Failed(String),
}

impl From<io::Error> for Refusal {
    fn from(e: io::Error) -> Self {
        Refusal::Failed(e.to_string())
    }
}

impl Refusal {
    fn into_message(self) -> String {
        match self {
            Refusal::Unsafe(what) => {
                format!("the archive is unsafe, so nothing was installed: {what}")
            }
            Refusal::Failed(what) => format!("could not extract archive: {what}"),
        }
    }
}

/// Unpacks `archive` into `staging`, an empty directory. `skip` are globs, relative to the build,
/// of what is left out (and everything below a directory that matches). Returns the bytes written.
pub fn extract(
    kind: ArchiveKind,
    archive: &Path,
    staging: &Path,
    skip: &[String],
) -> Result<u64, String> {
    let mut extractor =
        Extractor::new(staging, skip).map_err(|e| Refusal::from(e).into_message())?;
    let outcome = match kind {
        ArchiveKind::Zip => extract_zip(archive, &mut extractor),
        ArchiveKind::TarGz => File::open(archive)
            .map_err(Refusal::from)
            .and_then(|file| extract_tar(flate2::read::GzDecoder::new(file), &mut extractor)),
        ArchiveKind::TarZst => File::open(archive)
            .map_err(Refusal::from)
            .and_then(|file| extract_tar(ZstdReader::new(file), &mut extractor)),
    };
    outcome
        .map(|()| extractor.bytes)
        .map_err(Refusal::into_message)
}

fn extract_tar<R: Read>(source: R, extractor: &mut Extractor) -> Result<(), Refusal> {
    let mut archive = tar::Archive::new(source);
    extract_tar_entries(&mut archive, extractor)?;
    // `tar` stops at its end marker. What compression adds after it (the checksum of a zstd frame,
    // the CRC of a gzip member) and anything that should not be there is read now: an archive that
    // was damaged, or cut short, or has bytes tacked on, is not installed.
    io::copy(&mut archive.into_inner(), &mut io::sink())?;
    Ok(())
}

fn extract_tar_entries<R: Read>(
    archive: &mut tar::Archive<R>,
    extractor: &mut Extractor,
) -> Result<(), Refusal> {
    for entry in archive.entries()? {
        let mut entry = entry?;
        let kind = entry.header().entry_type();
        // the raw bytes of the name: what the archive says, before anything is made of it
        let name = entry.path()?.into_owned();
        use tar::EntryType as T;
        match kind {
            T::XGlobalHeader | T::XHeader => continue,
            T::Directory => extractor.directory(&name)?,
            T::Regular | T::GNUSparse => {
                let mode = entry.header().mode().ok();
                extractor.file(&name, mode, &mut entry)?;
            }
            T::Symlink => {
                let target = entry
                    .link_name()?
                    .map(|t| t.to_string_lossy().into_owned())
                    .unwrap_or_default();
                extractor.symlink(&name, &target)?;
            }
            T::Link => {
                return Err(Refusal::Unsafe(format!(
                    "`{}` is a hard link, which an install does not accept",
                    name.display()
                )))
            }
            other => {
                return Err(Refusal::Unsafe(format!(
                    "`{}` is neither a file, a directory nor a link ({other:?})",
                    name.display()
                )))
            }
        }
    }
    Ok(())
}

/// A zip's names are all in its directory at the end of the file, so the wrapping directory a
/// zip is often made with can be found before anything is written, and is removed.
fn extract_zip(archive: &Path, extractor: &mut Extractor) -> Result<(), Refusal> {
    let file = File::open(archive)?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| Refusal::Failed(e.to_string()))?;
    let wrapper = zip
        .root_dir(zip::read::root_dir_common_filter)
        .map_err(|e| Refusal::Failed(e.to_string()))?;
    for index in 0..zip.len() {
        let mut entry = zip
            .by_index(index)
            .map_err(|e| Refusal::Failed(e.to_string()))?;
        let name = PathBuf::from(entry.name());
        let name = match &wrapper {
            Some(wrapper) => match name.strip_prefix(wrapper) {
                Ok(inside) => inside.to_path_buf(),
                // outside the wrapper (a `__MACOSX` folder): it is not part of the build
                Err(_) => continue,
            },
            None => name,
        };
        if entry.is_dir() {
            extractor.directory(&name)?;
        } else if entry.is_symlink() {
            let mut target = String::new();
            entry
                .read_to_string(&mut target)
                .map_err(|e| Refusal::Failed(e.to_string()))?;
            extractor.symlink(&name, &target)?;
        } else {
            let mode = entry.unix_mode();
            extractor.file(&name, mode, &mut entry)?;
        }
    }
    Ok(())
}

struct Extractor {
    /// The staging directory as the file system names it (links resolved).
    root: PathBuf,
    skip: Vec<String>,
    bytes: u64,
    entries: u64,
}

impl Extractor {
    fn new(staging: &Path, skip: &[String]) -> io::Result<Self> {
        Ok(Extractor {
            root: std::fs::canonicalize(staging)?,
            skip: skip.to_vec(),
            bytes: 0,
            entries: 0,
        })
    }

    /// The path of an entry inside the build, without `.` and `..`; `None` for the build itself.
    /// A path that is absolute, or climbs above the build, or holds a name Windows would read as
    /// something else, is refused.
    fn clean(&self, name: &Path) -> Result<Option<PathBuf>, Refusal> {
        let shown = || name.display().to_string();
        let mut clean = PathBuf::new();
        for component in name.components() {
            match component {
                Component::Normal(part) => {
                    if !component_is_plain(part) {
                        return Err(Refusal::Unsafe(format!(
                            "`{}` has a name this system would read as something else",
                            shown()
                        )));
                    }
                    clean.push(part);
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    if !clean.pop() {
                        return Err(Refusal::Unsafe(format!(
                            "`{}` would be written outside the install directory",
                            shown()
                        )));
                    }
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(Refusal::Unsafe(format!(
                        "`{}` is an absolute path, so it would be written outside the install directory",
                        shown()
                    )));
                }
            }
        }
        Ok((!clean.as_os_str().is_empty()).then_some(clean))
    }

    fn count(&mut self) -> Result<(), Refusal> {
        self.entries += 1;
        if self.entries > MAX_ENTRIES {
            return Err(Refusal::Failed(format!(
                "the archive holds more than {MAX_ENTRIES} entries"
            )));
        }
        Ok(())
    }

    /// Whether `rel`, or a directory above it, is one of the globs to leave out.
    fn skipped(&self, rel: &Path) -> bool {
        if self.skip.is_empty() {
            return false;
        }
        let parts: Vec<String> = rel
            .components()
            .map(|c| c.as_os_str().to_string_lossy().into_owned())
            .collect();
        (1..=parts.len()).any(|n| {
            let prefix = parts[..n].join("/");
            self.skip.iter().any(|glob| glob_match(glob, &prefix))
        })
    }

    fn directory(&mut self, name: &Path) -> Result<(), Refusal> {
        self.count()?;
        let Some(rel) = self.clean(name)? else {
            return Ok(()); // the build's own directory
        };
        if self.skipped(&rel) {
            return Ok(());
        }
        self.ensure_dirs(&rel)
    }

    fn file(&mut self, name: &Path, mode: Option<u32>, data: &mut dyn Read) -> Result<(), Refusal> {
        self.count()?;
        let Some(rel) = self.clean(name)? else {
            return Err(Refusal::Unsafe(format!(
                "`{}` is a file with no name inside the install directory",
                name.display()
            )));
        };
        if self.skipped(&rel) {
            return Ok(());
        }
        let dest = self.destination(&rel)?;
        self.clear(&dest)?;
        let mut file = File::create(&dest)?;
        let room = MAX_UNPACKED_BYTES - self.bytes;
        let written = io::copy(&mut data.take(room + 1), &mut file)?;
        if written > room {
            return Err(Refusal::Failed(format!(
                "it unpacks to more than {} GiB",
                MAX_UNPACKED_BYTES >> 30
            )));
        }
        self.bytes += written;
        drop(file);
        set_mode(&dest, mode)?;
        Ok(())
    }

    fn symlink(&mut self, name: &Path, target: &str) -> Result<(), Refusal> {
        self.count()?;
        let Some(rel) = self.clean(name)? else {
            return Err(Refusal::Unsafe(format!(
                "`{}` is a link with no name inside the install directory",
                name.display()
            )));
        };
        // a link is judged whether or not it is kept: one that points out is the archive's fault
        if self.skipped(&rel) {
            // not made, so there is no directory to look at: judge it where it would sit
            let base = self.root.join(rel.parent().unwrap_or(Path::new("")));
            return self.check_link_target(&rel, base, target);
        }
        let dest = self.destination(&rel)?;
        let base = std::fs::canonicalize(dest.parent().unwrap_or(&self.root))?;
        self.check_link_target(&rel, base, target)?;
        self.clear(&dest)?;
        make_symlink(target, &dest)
    }

    /// A link's target must be relative, may climb with leading `..` only, and must land inside
    /// the build. `base` is the directory the link sits in as the file system names it (it may
    /// itself be reached through a link made earlier).
    fn check_link_target(
        &self,
        rel: &Path,
        mut place: PathBuf,
        target: &str,
    ) -> Result<(), Refusal> {
        let outside = |why: &str| {
            Refusal::Unsafe(format!(
                "the link `{}` -> `{target}` {why}, outside the install directory",
                rel.display()
            ))
        };
        if target.is_empty() || target.contains('\0') {
            return Err(outside("has no usable target"));
        }
        let mut seen_name = false;
        for component in Path::new(target).components() {
            match component {
                Component::ParentDir if !seen_name => {
                    if !place.pop() || !place.starts_with(&self.root) {
                        return Err(outside("climbs out"));
                    }
                }
                Component::ParentDir => {
                    return Err(outside(
                        "goes up again after going down, which cannot be told from climbing out here",
                    ));
                }
                Component::Normal(part) => {
                    seen_name = true;
                    if !component_is_plain(part) {
                        return Err(outside(
                            "names something this system would read differently",
                        ));
                    }
                    place.push(part);
                }
                Component::CurDir => {}
                Component::RootDir | Component::Prefix(_) => {
                    return Err(outside("is absolute and points"));
                }
            }
        }
        if place.starts_with(&self.root) {
            Ok(())
        } else {
            Err(outside("points"))
        }
    }

    /// The path to write `rel` at, with its directories made. Every directory on the way is
    /// resolved on disk and must be inside the build.
    fn destination(&self, rel: &Path) -> Result<PathBuf, Refusal> {
        if let Some(parent) = rel.parent() {
            self.ensure_dirs(parent)?;
        }
        Ok(self.root.join(rel))
    }

    /// Makes the directories of `rel` one by one. A part that is already a link is followed, and
    /// where it leads must be inside the build; a part that is a file is in the way.
    fn ensure_dirs(&self, rel: &Path) -> Result<(), Refusal> {
        let mut here = self.root.clone();
        for part in rel.components() {
            here.push(part);
            match std::fs::symlink_metadata(&here) {
                Ok(meta) if meta.is_dir() => {}
                Ok(meta) if meta.file_type().is_symlink() => {
                    let resolved = std::fs::canonicalize(&here).map_err(|_| {
                        Refusal::Failed(format!(
                            "a link of the archive ({}) points at nothing and is in the way of a directory",
                            here.display()
                        ))
                    })?;
                    if !resolved.starts_with(&self.root) || !resolved.is_dir() {
                        return Err(Refusal::Unsafe(format!(
                            "`{}` goes through a link that leads outside the install directory",
                            rel.display()
                        )));
                    }
                }
                Ok(_) => {
                    return Err(Refusal::Failed(format!(
                        "`{}` is a file where the archive also has a directory",
                        here.display()
                    )))
                }
                Err(e) if e.kind() == io::ErrorKind::NotFound => std::fs::create_dir(&here)?,
                Err(e) => return Err(e.into()),
            }
        }
        Ok(())
    }

    /// Takes away what is at `dest` so that a later entry replaces it, never writes through it.
    fn clear(&self, dest: &Path) -> Result<(), Refusal> {
        match std::fs::symlink_metadata(dest) {
            Ok(meta) if meta.is_dir() => Err(Refusal::Failed(format!(
                "`{}` is a directory where the archive also has a file",
                dest.display()
            ))),
            Ok(_) => Ok(std::fs::remove_file(dest)?),
            Err(_) => Ok(()),
        }
    }
}

/// A name that means the same here as it does in the archive. On Windows a colon makes an
/// alternate data stream or a drive, and the characters Windows reserves are not names.
fn component_is_plain(part: &std::ffi::OsStr) -> bool {
    let text = part.to_string_lossy();
    if text.contains('\0') {
        return false;
    }
    if cfg!(windows) {
        return !text
            .chars()
            .any(|c| matches!(c, '<' | '>' | ':' | '"' | '|' | '?' | '*') || c.is_control());
    }
    true
}

#[cfg(unix)]
fn set_mode(path: &Path, mode: Option<u32>) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    // setuid and the like are not kept; an archive that says nothing (or zero) gets an ordinary file
    let permissions = match mode.map(|m| m & 0o777) {
        None | Some(0) => 0o644,
        Some(bits) => bits,
    };
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(permissions))
}

#[cfg(not(unix))]
fn set_mode(_path: &Path, _mode: Option<u32>) -> io::Result<()> {
    Ok(())
}

#[cfg(unix)]
fn make_symlink(target: &str, dest: &Path) -> Result<(), Refusal> {
    Ok(std::os::unix::fs::symlink(target, dest)?)
}

#[cfg(not(unix))]
fn make_symlink(_target: &str, dest: &Path) -> Result<(), Refusal> {
    Err(Refusal::Failed(format!(
        "the link `{}` cannot be made on this system",
        dest.display()
    )))
}

/// Shell-style matching of a `/`-separated path: `*` is any run of characters inside one name,
/// `?` is one character, everything else stands for itself.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    fn go(p: &[char], t: &[char]) -> bool {
        match p.split_first() {
            None => t.is_empty(),
            Some(('*', rest)) => {
                (0..=t.len()).any(|n| t[..n].iter().all(|c| *c != '/') && go(rest, &t[n..]))
            }
            Some(('?', rest)) => t.first().is_some_and(|c| *c != '/') && go(rest, &t[1..]),
            Some((c, rest)) => t.first() == Some(c) && go(rest, &t[1..]),
        }
    }
    let (p, t): (Vec<char>, Vec<char>) = (pattern.chars().collect(), text.chars().collect());
    go(&p, &t)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_glob_matches_inside_one_name_and_never_across_a_slash() {
        assert!(glob_match("lib/ollama/cuda_*", "lib/ollama/cuda_v12"));
        assert!(glob_match("lib/ollama/cuda_*", "lib/ollama/cuda_"));
        assert!(!glob_match(
            "lib/ollama/cuda_*",
            "lib/ollama/cuda_v12/libx.so"
        ));
        assert!(!glob_match("lib/ollama/cuda_*", "lib/ollama/vulkan"));
        assert!(glob_match("*.so", "libx.so"));
        assert!(!glob_match("*.so", "lib/libx.so"));
        assert!(glob_match("a?c", "abc"));
        assert!(!glob_match("a?c", "a/c"));
        assert!(glob_match("exact", "exact"));
        assert!(!glob_match("exact", "exacta"));
        assert!(glob_match("*", "anything"));
        assert!(glob_match("", ""));
    }

    #[test]
    fn the_endings_an_installer_reads_are_told_apart_by_name_not_by_query() {
        use ArchiveKind::*;
        for (url, kind) in [
            ("https://x/a.tar.zst", Some(TarZst)),
            ("https://x/a.TAR.GZ", Some(TarGz)),
            ("https://x/a.tgz", Some(TarGz)),
            ("https://x/a.zip", Some(Zip)),
            ("https://x/a.zip?download=1", Some(Zip)),
            ("https://x/a.zip#frag", Some(Zip)),
            ("https://x/a.rar", None),
            ("https://x/a.zst", None),
            ("https://x/a.tar", None),
        ] {
            assert_eq!(ArchiveKind::from_url(url), kind, "{url}");
        }
        assert_eq!(TarZst.extension(), "tar.zst");
    }

    // ---- the extractor on archives made by hand ----------------------------------------------------

    use crate::testkit::archives::{tar_gz, zip, Entry};
    use crate::testkit::{scratch, Scratch};

    /// Unpacks `archive` (written to a file named `name`) into a fresh staging directory and gives
    /// back the outcome, the staging directory and the directory it was made in (whose contents
    /// show whether anything was written outside).
    fn unpack(
        name: &str,
        archive: Vec<u8>,
        skip: &[&str],
    ) -> (Result<u64, String>, PathBuf, Scratch) {
        let outer = scratch("extract");
        let file = outer.join(name);
        std::fs::write(&file, archive).unwrap();
        let staging = outer.join("staging");
        std::fs::create_dir(&staging).unwrap();
        let skip: Vec<String> = skip.iter().map(|s| s.to_string()).collect();
        let kind = ArchiveKind::from_url(name).unwrap();
        (extract(kind, &file, &staging, &skip), staging, outer)
    }

    fn names(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = std::fs::read_dir(dir)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[cfg(unix)]
    #[test]
    fn a_link_to_dot_does_not_make_a_later_link_look_inside_when_it_points_outside() {
        // `a -> .` is harmless; `a/l -> ../z` is judged where it will really sit, which is the
        // build's own directory, and from there `../z` leaves the build
        let (outcome, _staging, outer) = unpack(
            "chain.tar.gz",
            tar_gz(&[
                Entry::symlink("a", "."),
                Entry::symlink("a/l", "../z"),
                Entry::file("l/pwned.txt", b"x"),
            ]),
            &[],
        );
        let message = outcome.expect_err("a link that points out was accepted");
        assert!(
            message.contains("unsafe") && message.contains("outside"),
            "{message}"
        );
        assert_eq!(
            names(&outer),
            ["chain.tar.gz", "staging"],
            "something was written outside"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_directory_that_is_a_link_to_a_place_inside_is_fine_to_write_through() {
        let (outcome, staging, _outer) = unpack(
            "through.tar.gz",
            tar_gz(&[
                Entry::dir("real/"),
                Entry::symlink("alias", "real"),
                Entry::file("alias/x.txt", b"hello"),
            ]),
            &[],
        );
        outcome.expect("a link to a directory inside the build is fine");
        assert_eq!(std::fs::read(staging.join("real/x.txt")).unwrap(), b"hello");
    }

    #[cfg(unix)]
    #[test]
    fn a_link_that_leaves_the_build_through_an_earlier_link_is_refused() {
        // `up` is a link to the build itself (fine); a file written below `up/..` would be outside
        let (outcome, _staging, outer) = unpack(
            "up.tar.gz",
            tar_gz(&[
                Entry::dir("sub/"),
                Entry::symlink("sub/up", ".."),
                Entry::symlink("sub/up/up", ".."),
                Entry::file("sub/up/up/escaped.txt", b"x"),
            ]),
            &[],
        );
        assert!(outcome.is_err(), "a way out through two links was accepted");
        assert_eq!(names(&outer), ["staging", "up.tar.gz"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_later_entry_replaces_an_earlier_one_and_never_writes_through_a_link() {
        let (outcome, staging, _outer) = unpack(
            "again.tar.gz",
            tar_gz(&[
                Entry::file("data", b"original"),
                Entry::symlink("alias", "data"),
                // a file where a link is: the link goes, the file is written, `data` is untouched
                Entry::file("alias", b"replacement"),
                Entry::file("data", b"second version"),
            ]),
            &[],
        );
        outcome.expect("installs");
        assert_eq!(
            std::fs::read(staging.join("alias")).unwrap(),
            b"replacement"
        );
        assert!(!std::fs::symlink_metadata(staging.join("alias"))
            .unwrap()
            .file_type()
            .is_symlink());
        assert_eq!(
            std::fs::read(staging.join("data")).unwrap(),
            b"second version"
        );
    }

    #[test]
    fn a_name_with_dots_in_the_middle_is_kept_when_it_stays_inside() {
        let (outcome, staging, _outer) = unpack(
            "dots.tar.gz",
            tar_gz(&[
                Entry::file("a/../b.txt", b"x"),
                Entry::file("./c.txt", b"y"),
            ]),
            &[],
        );
        outcome.expect("a path that stays inside is fine");
        assert_eq!(names(&staging), ["b.txt", "c.txt"]);
    }

    #[cfg(unix)]
    #[test]
    fn a_file_where_the_archive_also_has_a_directory_is_an_error_not_a_panic() {
        for entries in [
            vec![Entry::file("x", b"1"), Entry::file("x/y", b"2")],
            vec![Entry::file("x/y", b"2"), Entry::file("x", b"1")],
        ] {
            let (outcome, _staging, _outer) = unpack("clash.tar.gz", tar_gz(&entries), &[]);
            assert!(outcome.is_err());
        }
    }

    #[cfg(unix)]
    #[test]
    fn permissions_are_the_archives_without_the_bits_that_would_be_dangerous() {
        use std::os::unix::fs::PermissionsExt;
        let entry = |path: &str, mode: u32| Entry {
            kind: crate::testkit::archives::Kind::File { mode },
            ..Entry::file(path, b"x")
        };
        let (outcome, staging, _outer) = unpack(
            "modes.tar.gz",
            tar_gz(&[
                entry("run", 0o755),
                entry("setuid", 0o4755),
                entry("blank", 0),
                entry("private", 0o600),
            ]),
            &[],
        );
        outcome.expect("installs");
        let mode = |n: &str| {
            std::fs::metadata(staging.join(n))
                .unwrap()
                .permissions()
                .mode()
                & 0o7777
        };
        assert_eq!(mode("run"), 0o755);
        assert_eq!(mode("setuid"), 0o755, "setuid is not kept");
        assert_eq!(
            mode("blank"),
            0o644,
            "an archive that says nothing gets an ordinary file"
        );
        assert_eq!(mode("private"), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn a_skipped_link_is_still_judged_and_a_skipped_directory_takes_what_is_below_it() {
        let (outcome, staging, _outer) = unpack(
            "skip.tar.gz",
            tar_gz(&[
                Entry::file("keep.txt", b"k"),
                Entry::dir("cuda_v12/"),
                Entry::file("cuda_v12/lib.so", b"c"),
                Entry::symlink("cuda_v12/lib.so.12", "lib.so"),
            ]),
            &["cuda_*"],
        );
        outcome.expect("installs");
        assert_eq!(names(&staging), ["keep.txt"]);
        // a hostile link inside what is skipped is the archive's fault all the same
        let (outcome, _staging, _outer) = unpack(
            "skip-bad.tar.gz",
            tar_gz(&[Entry::symlink("cuda_v12/evil", "../../etc")]),
            &["cuda_*"],
        );
        assert!(outcome.is_err());
    }

    #[test]
    fn a_zip_with_a_wrapping_directory_is_unwrapped_and_junk_beside_it_is_dropped() {
        let (outcome, staging, _outer) = unpack(
            "wrapped.zip",
            zip(&[
                Entry::dir("llama-b1/"),
                Entry::file("llama-b1/llama-server", b"bin"),
                Entry::file("llama-b1/lib/libggml.so", b"lib"),
                Entry::file("__MACOSX/llama-b1/._llama-server", b"junk"),
            ]),
            &[],
        );
        outcome.expect("installs");
        assert_eq!(names(&staging), ["lib", "llama-server"]);
        // two top-level directories are not a wrapper
        let (outcome, staging, _outer) = unpack(
            "two.zip",
            zip(&[Entry::file("a/x", b"1"), Entry::file("b/y", b"2")]),
            &[],
        );
        outcome.expect("installs");
        assert_eq!(names(&staging), ["a", "b"]);
    }

    #[test]
    fn a_zip_entry_that_climbs_out_refuses_the_zip() {
        let (outcome, _staging, outer) = unpack(
            "evil.zip",
            zip(&[
                Entry::file("ok.txt", b"1"),
                Entry::file("../escaped.txt", b"2"),
            ]),
            &[],
        );
        let message = outcome.expect_err("a zip entry that climbs out was accepted");
        assert!(message.contains("unsafe"), "{message}");
        assert_eq!(names(&outer), ["evil.zip", "staging"]);
    }
}
