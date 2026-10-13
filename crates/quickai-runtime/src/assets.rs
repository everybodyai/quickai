//! The files that ship inside the application and are put on the disk for a program to read
//! (`[[launch.spawn.write]]` of an engine document, docs/comfyui.md).
//!
//! They are compiled in from `crates/quickai-runtime/assets`. A document names one by its path there
//! (`comfyui/quickai_bridge`: a folder, with everything in it) and says where it goes; the supervisor
//! calls `put` before it starts the program, every time, so that an update of the application is an
//! update of them and a build that was made long ago has them as a new one does.

use std::io::Write;
use std::path::{Path, PathBuf};

use include_dir::{Dir, DirEntry, File};

static ASSETS: Dir<'static> = include_dir::include_dir!("$CARGO_MANIFEST_DIR/assets");

/// What `put` did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct Put {
    /// The files that were not there, or were not the same, and were written.
    pub written: usize,
    /// The files that were there and the same, and were left alone.
    pub kept: usize,
}

/// Whether the application has a file or folder of this name.
#[cfg(test)]
pub fn has(name: &str) -> bool {
    entry(name).is_some()
}

/// The names of every file and folder the application has, which is what a document is held to when it loads.
pub fn names() -> Vec<String> {
    fn walk(dir: &Dir<'static>, out: &mut Vec<String>) {
        for entry in dir.entries().iter().filter(|e| !is_cache(e.path())) {
            out.push(entry.path().display().to_string().replace('\\', "/"));
            if let DirEntry::Dir(inner) = entry {
                walk(inner, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(&ASSETS, &mut out);
    out
}

fn entry(name: &str) -> Option<&'static DirEntry<'static>> {
    if name.is_empty()
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return None;
    }
    ASSETS.get_entry(name).filter(|e| !is_cache(e.path()))
}

/// What Python writes beside a file it has run (`__pycache__/x.pyc`) is not part of the file: a run of the
/// tests on the machine the application is built on must not put it in the application, or on a person's.
fn is_cache(path: &Path) -> bool {
    path.components().any(|c| c.as_os_str() == "__pycache__")
        || path.extension().is_some_and(|e| e == "pyc")
}

/// Puts the file, or the folder and everything in it, at `to`: a file that is there and has the same
/// content is left alone, any other is written whole (a temporary file, then a rename), and a file that
/// is there and is not one of the application's is not touched.
pub fn put(name: &str, to: &Path) -> std::io::Result<Put> {
    let Some(entry) = entry(name) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            format!("the application has no file called {name}"),
        ));
    };
    let mut done = Put::default();
    match entry {
        DirEntry::File(file) => put_file(file, to, &mut done)?,
        DirEntry::Dir(dir) => put_dir(dir, name, to, &mut done)?,
    }
    Ok(done)
}

fn put_dir(dir: &Dir<'static>, root: &str, to: &Path, done: &mut Put) -> std::io::Result<()> {
    for entry in dir.entries().iter().filter(|e| !is_cache(e.path())) {
        match entry {
            DirEntry::File(file) => {
                let relative = relative(file.path(), root);
                put_file(file, &to.join(relative), done)?;
            }
            DirEntry::Dir(inner) => put_dir(inner, root, to, done)?,
        }
    }
    Ok(())
}

/// A path inside the asset: what comes after the folder that was asked for.
fn relative(path: &Path, root: &str) -> PathBuf {
    path.strip_prefix(root).unwrap_or(path).to_path_buf()
}

fn put_file(file: &File<'static>, to: &Path, done: &mut Put) -> std::io::Result<()> {
    if std::fs::read(to).is_ok_and(|there| there == file.contents()) {
        done.kept += 1;
        return Ok(());
    }
    if let Some(parent) = to.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut name = to.file_name().unwrap_or_default().to_os_string();
    name.push(".quickai-new");
    let temporary = to.with_file_name(name);
    let mut out = std::fs::File::create(&temporary)?;
    out.write_all(file.contents())?;
    out.sync_all()?;
    drop(out);
    if let Err(e) = std::fs::rename(&temporary, to) {
        let _ = std::fs::remove_file(&temporary);
        return Err(e);
    }
    done.written += 1;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "quickai-assets-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn p3_p15_a_folder_of_the_application_is_put_whole_and_what_is_the_same_is_left_alone() {
        let to = scratch("whole").join("extensions").join("quickai_bridge");
        let first = put("comfyui/quickai_bridge", &to).unwrap();
        assert!(first.written >= 3 && first.kept == 0, "{first:?}");
        for file in ["__init__.py", "fetch.py", "web/quickai_bridge.js"] {
            assert!(to.join(file).is_file(), "{file}");
        }
        assert_eq!(
            std::fs::read(to.join("fetch.py")).unwrap(),
            include_bytes!("../assets/comfyui/quickai_bridge/fetch.py")
        );
        let second = put("comfyui/quickai_bridge", &to).unwrap();
        assert_eq!(
            second,
            Put {
                written: 0,
                kept: first.written
            }
        );
        assert!(
            std::fs::read_dir(&to).unwrap().all(|e| !e
                .unwrap()
                .file_name()
                .to_string_lossy()
                .ends_with(".quickai-new")),
            "no temporary file is left"
        );
    }

    #[test]
    fn p3_p16_a_file_that_differs_is_written_again_and_one_that_is_not_the_applications_is_not_touched(
    ) {
        let to = scratch("differs").join("quickai_bridge");
        put("comfyui/quickai_bridge", &to).unwrap();
        std::fs::write(to.join("fetch.py"), "an older one").unwrap();
        std::fs::create_dir_all(to.join("__pycache__")).unwrap();
        std::fs::write(to.join("__pycache__").join("fetch.pyc"), "compiled").unwrap();
        std::fs::write(to.join("mine.txt"), "the person's").unwrap();
        let again = put("comfyui/quickai_bridge", &to).unwrap();
        assert_eq!(again.written, 1, "{again:?}");
        assert_eq!(
            std::fs::read(to.join("fetch.py")).unwrap(),
            include_bytes!("../assets/comfyui/quickai_bridge/fetch.py")
        );
        assert_eq!(
            std::fs::read_to_string(to.join("mine.txt")).unwrap(),
            "the person's"
        );
        assert_eq!(
            std::fs::read_to_string(to.join("__pycache__").join("fetch.pyc")).unwrap(),
            "compiled"
        );
    }

    #[test]
    fn p3_p17_a_file_is_put_where_it_is_told_and_a_name_the_application_lacks_or_that_climbs_is_no_file(
    ) {
        let to = scratch("file").join("deep").join("extra_model_paths.yaml");
        assert_eq!(
            put("comfyui/extra_model_paths.yaml", &to).unwrap().written,
            1
        );
        assert!(std::fs::read_to_string(&to)
            .unwrap()
            .contains("custom_nodes: extensions"));
        let error = put("comfyui/nothing", &to).unwrap_err();
        assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
        assert!(error.to_string().contains("comfyui/nothing"), "{error}");
        for bad in [
            "",
            "..",
            "comfyui/../comfyui",
            "/comfyui",
            "comfyui//quickai_bridge",
            "./comfyui",
        ] {
            assert!(!has(bad), "{bad:?}");
            assert!(put(bad, &to).is_err(), "{bad:?}");
        }
        assert!(
            !has("comfyui/quickai_bridge/__pycache__"),
            "what Python writes is no part of it"
        );
        assert!(is_cache(Path::new(
            "comfyui/quickai_bridge/__pycache__/fetch.cpython-314.pyc"
        )));
        assert!(is_cache(Path::new("comfyui/quickai_bridge/fetch.pyc")));
        assert!(!is_cache(Path::new("comfyui/quickai_bridge/fetch.py")));
        assert!(has("comfyui/quickai_bridge/__init__.py"), "a file");
        assert!(has("comfyui/quickai_bridge"), "a folder");
        // what a document may name is every file and folder, and nothing else
        let names = names();
        for name in [
            "comfyui",
            "comfyui/quickai_bridge",
            "comfyui/quickai_bridge/web/quickai_bridge.js",
        ] {
            assert!(names.iter().any(|n| n == name), "{name}: {names:?}");
        }
    }
}
