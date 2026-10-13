//! The runtime is QuickAI without a window (docs/headless.md, WP4.18): the window and the headless program
//! are both its clients, and neither is in it. `quickai-core` is held to being pure by `purity.rs`; this
//! holds the layer above it to being windowless, and the window to being only a window.
//!
//! Like that test it is a grep, not a proof, and it turns the rule from a convention into something a pull
//! request cannot break unnoticed: a program on a server that has no display must not have to carry, link or
//! initialise a toolkit to install a model.

use std::path::{Path, PathBuf};

/// The crates of a window: a dependency on one of them in the runtime would put a toolkit into every
/// program that uses it.
const TOOLKITS: &[&str] = &[
    "eframe",
    "egui",
    "egui_glow",
    "egui-wgpu",
    "epaint",
    "winit",
    "wgpu",
    "glow",
    "glutin",
    "rfd",
    "webbrowser",
    "gtk",
    "gtk4",
    "tao",
    "slint",
    "iced",
];

/// Paths that mean "a window", in the code (a name that merely contains a crate's name is not one).
const WINDOW_PATHS: &[&str] = &[
    "egui::",
    "eframe::",
    "rfd::",
    "webbrowser::",
    "winit::",
    "wgpu::",
];

fn crate_dir(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join(name)
}

/// The names a manifest depends on, in any dependency table (`[dependencies]`, `[dev-dependencies]`,
/// `[target.'cfg(unix)'.dependencies]`, `[dependencies.name]`).
fn dependencies_of(manifest: &str) -> Vec<String> {
    let mut names = Vec::new();
    let mut in_dependencies = false;
    for raw in manifest.lines() {
        let line = raw.trim();
        if line.starts_with('[') {
            let header = line.trim_matches(|c| c == '[' || c == ']');
            in_dependencies = header.ends_with("dependencies");
            if let Some((_, name)) = header.split_once("dependencies.") {
                names.push(name.trim_matches('"').to_string());
            }
        } else if in_dependencies {
            if let Some((name, _)) = line.split_once('=') {
                let name = name.trim();
                if !name.is_empty() && !name.starts_with('#') {
                    names.push(name.trim_matches('"').to_string());
                }
            }
        }
    }
    names
}

/// The lines of `source` that mention a window path outside a comment, as `(line number, path)`.
fn window_paths_in(source: &str) -> Vec<(usize, &'static str)> {
    let mut found = Vec::new();
    let mut in_block_comment = false;
    for (index, raw) in source.lines().enumerate() {
        let mut line = raw;
        if in_block_comment {
            match line.find("*/") {
                Some(end) => {
                    line = &line[end + 2..];
                    in_block_comment = false;
                }
                None => continue,
            }
        }
        if let Some(start) = line.find("/*") {
            if !line[start..].contains("*/") {
                in_block_comment = true;
            }
            line = &line[..start];
        }
        let code = line.find("//").map_or(line, |at| &line[..at]);
        for path in WINDOW_PATHS {
            // `egui::` in `my_egui::` is not the toolkit
            let at = code.find(path);
            let boundary = at.is_some_and(|at| {
                at == 0
                    || !code.as_bytes()[at - 1].is_ascii_alphanumeric()
                        && code.as_bytes()[at - 1] != b'_'
            });
            if boundary {
                found.push((index + 1, *path));
            }
        }
    }
    found
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    for entry in std::fs::read_dir(dir).expect("readable source directory") {
        let path = entry.expect("directory entry").path();
        if path.is_dir() {
            rust_files(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn the_scanners_catch_a_planted_toolkit() {
    let manifest = "[dependencies]\nserde = \"1\"\neframe = \"=0.36.1\"\n\n[target.'cfg(unix)'.dependencies]\nrfd = \"0.17\"\n\n[dependencies.webbrowser]\nversion = \"1\"\n\n[package]\nname = \"egui\"\n";
    let names = dependencies_of(manifest);
    assert_eq!(names, ["serde", "eframe", "rfd", "webbrowser"]);
    assert_eq!(
        window_paths_in(
            "use eframe::egui;\nlet x = my_egui::Thing;\nlet y = rfd::FileDialog::new();\n"
        ),
        vec![(1, "eframe::"), (3, "rfd::")]
    );
    assert!(window_paths_in(
        "// eframe::run_native is the window's\n/// egui:: draws\nfn f() {}\n"
    )
    .is_empty());
    assert!(window_paths_in("/* webbrowser::open\n   winit:: */\nfn f() {}\n").is_empty());
}

/// O01: no toolkit is a dependency of the runtime, and none is named in its code.
#[test]
fn p3_o01_the_runtime_names_no_toolkit() {
    let dir = crate_dir("quickai-runtime");
    let manifest = std::fs::read_to_string(dir.join("Cargo.toml")).expect("the manifest");
    let wrong: Vec<String> = dependencies_of(&manifest)
        .into_iter()
        .filter(|name| TOOLKITS.contains(&name.as_str()))
        .collect();
    assert!(
        wrong.is_empty(),
        "quickai-runtime depends on {wrong:?}: a toolkit belongs to the window (crates/quickai)"
    );

    let mut files = Vec::new();
    rust_files(&dir.join("src"), &mut files);
    rust_files(&dir.join("tests"), &mut files);
    assert!(
        files.len() > 40,
        "the sources were not found: {}",
        files.len()
    );
    let mut report = Vec::new();
    for file in files {
        if file.file_name().is_some_and(|n| n == "no_window.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("readable source file");
        for (line, path) in window_paths_in(&text) {
            report.push(format!("{}:{line}: {path}", file.display()));
        }
    }
    assert!(
        report.is_empty(),
        "quickai-runtime must stay windowless; drawing belongs in the `quickai` crate:\n{}",
        report.join("\n")
    );
}

/// O02: the window crate holds a window and nothing else: its source is `main.rs` and the `app` folder, so
/// that what was done to the computer is in one place and a second program does not have to copy it.
#[test]
fn p3_o02_the_window_crate_holds_only_the_window() {
    let src = crate_dir("quickai").join("src");
    let mut names: Vec<String> = std::fs::read_dir(&src)
        .expect("the window's sources")
        .map(|e| e.expect("entry").file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        ["app", "main.rs"],
        "the window crate has something that is not the window: it belongs in quickai-runtime"
    );
    let manifest =
        std::fs::read_to_string(crate_dir("quickai").join("Cargo.toml")).expect("manifest");
    let depends = dependencies_of(&manifest);
    assert!(
        depends.iter().any(|d| d == "quickai-runtime"),
        "the window does what it does to the computer through the runtime: {depends:?}"
    );
    for engine_only in [
        "tiny_http",
        "tar",
        "zip",
        "flate2",
        "ruzstd",
        "sha2",
        "sysinfo",
        "include_dir",
        "regex",
    ] {
        assert!(
            !depends.iter().any(|d| d == engine_only),
            "the window depends on `{engine_only}`, which is how the runtime installs and serves: it is the runtime's"
        );
    }
}
