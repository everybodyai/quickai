//! The core must stay pure (docs/implementation-plan.md, WP1.1): no processes, no sockets, no
//! filesystem, no clock, no environment, no threads, no UI. Configuration is read through a
//! `LayerSource` the caller supplies, so tests of the core use in-memory sources.
//!
//! This test reads every source file of the crate and rejects the forbidden paths. It is a grep, not
//! a proof, but it turns the rule from a convention into something a pull request cannot break
//! unnoticed.

use std::path::{Path, PathBuf};

/// Paths that mean "an effect". A new crate dependency that performs effects would also have to be
/// named here, which is the point of having the list in one visible place. Crates are matched by
/// their path prefix (`ureq::`), so a word that merely contains a crate's name is not a violation.
const FORBIDDEN: &[&str] = &[
    "std::process",
    "std::net",
    "std::fs",
    "std::env",
    "std::thread",
    "std::time::Instant",
    "std::time::SystemTime",
    "std::io::stdin",
    "std::io::stdout",
    "std::io::stderr",
    "println!",
    "eprintln!",
    "dbg!",
    "ureq::",
    "tiny_http::",
    "egui::",
    "eframe::",
    "crossbeam_channel::",
    "crossbeam::",
    "sysinfo::",
    "directories::",
    "rfd::",
    "webbrowser::",
    "unsafe ",
];

/// The lines of `source` that mention a forbidden path outside a comment, as `(line number, path)`.
fn violations(source: &str) -> Vec<(usize, &'static str)> {
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
        let code = match line.find("//") {
            Some(at) => &line[..at],
            None => line,
        };
        for path in FORBIDDEN {
            if code.contains(path) {
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
fn the_scanner_catches_a_planted_violation() {
    assert_eq!(
        violations("fn read() { let _ = std::fs::read_to_string(\"x\"); }\n"),
        vec![(1, "std::fs")]
    );
    assert_eq!(
        violations("use std::process::Command;\nuse eframe::egui;\nuse directories::BaseDirs;\n"),
        vec![(1, "std::process"), (2, "eframe::"), (3, "directories::")]
    );
    assert_eq!(violations("println!(\"hi\");\n"), vec![(1, "println!")]);
    assert_eq!(
        violations("let t = std::time::Instant::now();\n"),
        vec![(1, "std::time::Instant")]
    );
}

#[test]
fn the_scanner_ignores_comments() {
    assert!(violations("// std::fs is how a caller reads files\n").is_empty());
    assert!(violations("/// uses std::process elsewhere\nfn f() {}\n").is_empty());
    assert!(violations("/* std::net\n   std::env */\nfn f() {}\n").is_empty());
    assert!(violations("let x = 1; // std::thread\n").is_empty());
}

#[test]
fn the_core_sources_are_pure() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut files = Vec::new();
    rust_files(&src, &mut files);
    assert!(
        !files.is_empty(),
        "no sources found under {}",
        src.display()
    );

    let mut report = Vec::new();
    for file in files {
        let text = std::fs::read_to_string(&file).expect("readable source file");
        for (line, path) in violations(&text) {
            report.push(format!("{}:{line}: {path}", file.display()));
        }
    }
    assert!(
        report.is_empty(),
        "quickai-core must stay pure; effects belong in the `quickai` crate:\n{}",
        report.join("\n")
    );
}

/// A33 of docs/phase3-test-plan.md: reading, checking, previewing and exporting a configuration use no
/// network, start no program and write no file. They are the core's, given what they need to know
/// (`profile::check::Inventory`), and the core is held to the list above.
#[test]
fn p3_a33_reading_checking_and_exporting_a_configuration_do_no_effect() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("src")
        .join("profile");
    let mut files = Vec::new();
    rust_files(&dir, &mut files);
    let names: Vec<String> = files
        .iter()
        .filter_map(|f| f.file_name().map(|n| n.to_string_lossy().into_owned()))
        .collect();
    for wanted in ["mod.rs", "check.rs", "notes.rs", "screen.rs", "state.rs"] {
        assert!(
            names.iter().any(|n| n == wanted),
            "{wanted} is missing from {names:?}"
        );
    }
    for file in files {
        let source = std::fs::read_to_string(&file).expect("readable source");
        let found = violations(&source);
        assert!(found.is_empty(), "{}: {found:?}", file.display());
    }
}
