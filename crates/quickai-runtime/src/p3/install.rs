//! Group Z of the test plan: `install.archive@1` grown up (work package 3.1). Every test here
//! goes through the installer's one entry point, `spawn_install`, with an archive served by a real
//! local server, so what is checked is what a person's click would do.

use std::path::{Path, PathBuf};
use std::time::Duration;

#[cfg(unix)]
use crate::jobs::JobEvent;
#[cfg(unix)]
use crate::prims::install::{install_blocking, InstallRequest, Smoke, Source};
use crate::prims::install::{list_installed, spawn_install, InstallSpec};
#[cfg(unix)]
use crate::testkit::archives::{
    assert_extracted, fixture, ollama_linux_tree, ollama_linux_tree as linux_tree, ollama_mac_tree,
    pseudo, sha256_hex, zst_frame, zst_skippable,
};
use crate::testkit::archives::{
    assert_extracted_content, ollama_windows_tree, tar_gz, tar_zst, zip, Entry,
};
use crate::testkit::files::FileServer;
use crate::testkit::{procs, scratch};

/// The one place that knows how an `InstallSpec` is built; the work package adds fields to it.
fn spec(root: &Path, binary: &str) -> InstallSpec {
    InstallSpec {
        root: root.to_path_buf(),
        binary: binary.to_string(),
        search_depth: 4,
        chmod: true,
        fallback_label: "ollama-build".into(),
        ..Default::default()
    }
}

fn ollama_spec(root: &Path) -> InstallSpec {
    spec(root, &crate::prims::install::binary_file_name("ollama"))
}

fn install(spec: &InstallSpec, url: &str, label: &str) -> Result<PathBuf, String> {
    spawn_install(spec.clone(), url.to_string(), label.to_string(), || {})
        .wait(Duration::from_secs(60))
        .expect("the install finished within a minute")
}

/// What is directly under `root`, sorted: the staging files of an install show here.
fn entries(root: &Path) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(root)
        .map(|rd| {
            rd.flatten()
                .map(|e| e.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
}

/// The layout of the real-zstd fixtures: path, size (links and directories have none).
#[cfg(unix)]
const MINI: &[(&str, u64)] = &[
    ("bin/ollama", 252),
    ("lib/ollama/GO_LICENSE", 30),
    ("lib/ollama/cuda_v12/libggml-cuda.so", 24_000),
    ("lib/ollama/cuda_v12/libcudart.so.12.8.90", 4_000),
    ("lib/ollama/cuda_v13/libggml-cuda.so", 24_000),
    ("lib/ollama/libggml-base.so.0.25.3", 6_000),
    ("lib/ollama/libggml-cpu-x64.so", 6_000),
    ("lib/ollama/llama-server", 17),
    ("lib/ollama/vulkan/libggml-vulkan.so", 16_000),
];
#[cfg(unix)]
const MINI_LINKS: &[(&str, &str)] = &[
    ("lib/ollama/libggml-base.so", "libggml-base.so.0"),
    ("lib/ollama/libggml-base.so.0", "libggml-base.so.0.25.3"),
    (
        "lib/ollama/cuda_v12/libcudart.so.12",
        "libcudart.so.12.8.90",
    ),
];

#[cfg(unix)]
fn assert_mini_layout(dir: &Path) {
    for (path, size) in MINI {
        let meta = std::fs::metadata(dir.join(path))
            .unwrap_or_else(|e| panic!("{path} was not extracted: {e}"));
        assert_eq!(meta.len(), *size, "{path}: size");
    }
    #[cfg(unix)]
    for (path, target) in MINI_LINKS {
        let got = std::fs::read_link(dir.join(path)).unwrap_or_else(|e| panic!("{path}: {e}"));
        assert_eq!(got.to_string_lossy(), *target, "{path}: link target");
    }
}

// ---- formats ---------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_z01_a_tar_zst_with_the_real_layout_is_installed_and_runs() {
    let root = scratch("z01");
    let server = FileServer::start();
    server.add("/ollama-linux-amd64.tar.zst", tar_zst(&ollama_linux_tree()));
    let binary = install(
        &ollama_spec(&root),
        &server.url("/ollama-linux-amd64.tar.zst"),
        "v0.35.1",
    )
    .expect("a .tar.zst installs");
    assert_eq!(binary, root.join("v0.35.1").join("bin").join("ollama"));
    assert_extracted(&root.join("v0.35.1"), &ollama_linux_tree());
    // the executable bit survived, and what was installed is what runs
    let out = std::process::Command::new(&binary)
        .arg("--version")
        .output()
        .unwrap();
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "ollama version is 0.35.1"
    );
    assert_eq!(
        entries(&root),
        ["v0.35.1"],
        "nothing but the build is left in the root"
    );
}

#[cfg(unix)]
#[test]
fn p3_z02_archives_made_by_the_real_zstd_install_the_same_whatever_their_framing() {
    let server = FileServer::start();
    for name in [
        "mini-ollama-linux.tar.zst",
        "mini-ollama-linux-nocheck.tar.zst",
        "mini-ollama-linux-multiframe.tar.zst",
    ] {
        let root = scratch("z02");
        server.add(&format!("/{name}"), fixture(name));
        install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1")
            .unwrap_or_else(|e| panic!("{name}: {e}"));
        assert_mini_layout(&root.join("v1"));
    }
}

#[cfg(unix)]
#[test]
fn p3_z03_a_tgz_of_the_mac_layout_is_installed() {
    let root = scratch("z03");
    let server = FileServer::start();
    server.add("/ollama-darwin.tgz", tar_gz(&ollama_mac_tree()));
    let binary = install(
        &ollama_spec(&root),
        &server.url("/ollama-darwin.tgz"),
        "v0.35.1",
    )
    .expect("a .tgz installs");
    assert_eq!(binary, root.join("v0.35.1").join("ollama"));
    assert_extracted(&root.join("v0.35.1"), &ollama_mac_tree());
}

#[test]
fn p3_z04_a_zip_of_the_windows_layout_is_installed_with_its_binary_at_the_root() {
    let root = scratch("z04");
    let server = FileServer::start();
    server.add("/ollama-windows-amd64.zip", zip(&ollama_windows_tree()));
    let binary = install(
        &spec(&root, "ollama.exe"),
        &server.url("/ollama-windows-amd64.zip"),
        "v0.35.1",
    )
    .expect("the Windows zip installs");
    assert_eq!(binary, root.join("v0.35.1").join("ollama.exe"));
    assert_extracted_content(&root.join("v0.35.1"), &ollama_windows_tree());
}

#[cfg(unix)]
#[test]
fn p3_z22_an_archive_with_no_entries_for_its_parent_directories_still_extracts() {
    // the real archive has no entry for `bin/` or `lib/ollama/`: the extractor makes them
    let root = scratch("z22");
    let server = FileServer::start();
    server.add("/o.tar.gz", tar_gz(&ollama_linux_tree()));
    install(&ollama_spec(&root), &server.url("/o.tar.gz"), "v1").expect("installs");
    assert_extracted(&root.join("v1"), &ollama_linux_tree());
}

// ---- damaged archives ------------------------------------------------------------------------------
//
// Each starts from the intact archive, which must install: a damaged archive is refused for being
// damaged, not for being a kind the installer does not read.

#[cfg(unix)]
fn assert_refused_and_nothing_left(
    root: &Path,
    outcome: Result<PathBuf, String>,
    what: &str,
) -> String {
    let message = outcome.expect_err(&format!("{what}: a damaged archive was installed"));
    assert!(
        entries(root).is_empty(),
        "{what}: the install left {:?} behind",
        entries(root)
    );
    message
}

#[cfg(unix)]
#[test]
fn p3_z05_a_truncated_tar_zst_says_it_is_incomplete_and_leaves_nothing() {
    let good = tar_zst(&ollama_linux_tree());
    let server = FileServer::start();
    server.add("/good.tar.zst", good.clone());
    install(
        &ollama_spec(&scratch("z05-control")),
        &server.url("/good.tar.zst"),
        "v1",
    )
    .expect("the intact archive installs");
    for cut in [0, 3, 10, good.len() / 2, good.len() - 1] {
        let root = scratch("z05");
        server.add("/cut.tar.zst", good[..cut].to_vec());
        let message = assert_refused_and_nothing_left(
            &root,
            install(&ollama_spec(&root), &server.url("/cut.tar.zst"), "v1"),
            &format!("cut at {cut}"),
        );
        let lower = message.to_lowercase();
        assert!(
            lower.contains("incomplete")
                || lower.contains("truncated")
                || lower.contains("cut short"),
            "cut at {cut}: the message does not say the archive is cut short: {message}"
        );
    }
}

#[cfg(unix)]
#[test]
fn p3_z06_a_damaged_tar_zst_is_refused_by_its_checksum_and_leaves_nothing() {
    // made by the real zstd with an XXH64 checksum, so a flipped byte anywhere in the data shows
    let good = fixture("mini-ollama-linux.tar.zst");
    let server = FileServer::start();
    server.add("/good.tar.zst", good.clone());
    install(
        &ollama_spec(&scratch("z06-control")),
        &server.url("/good.tar.zst"),
        "v1",
    )
    .expect("the intact archive installs");
    for at in [good.len() / 3, good.len() / 2, good.len() - 6] {
        let mut bad = good.clone();
        bad[at] ^= 0x55;
        let root = scratch("z06");
        server.add("/bad.tar.zst", bad);
        assert_refused_and_nothing_left(
            &root,
            install(&ollama_spec(&root), &server.url("/bad.tar.zst"), "v1"),
            &format!("byte {at} flipped"),
        );
    }
}

#[cfg(unix)]
#[test]
fn p3_z07_garbage_after_the_last_frame_is_refused_and_a_skippable_frame_before_it_is_not() {
    let tar_bytes = crate::testkit::archives::tar(&ollama_linux_tree());
    let server = FileServer::start();

    let mut with_skippable = zst_skippable(b"metadata the reader must skip");
    with_skippable.extend(zst_frame(&tar_bytes));
    server.add("/skip.tar.zst", with_skippable);
    let root = scratch("z07-skip");
    install(&ollama_spec(&root), &server.url("/skip.tar.zst"), "v1")
        .expect("a skippable frame is skipped");
    assert_extracted(&root.join("v1"), &ollama_linux_tree());

    let mut with_garbage = zst_frame(&tar_bytes);
    with_garbage.extend(b"this is not a zstd frame");
    server.add("/garbage.tar.zst", with_garbage);
    let root = scratch("z07-garbage");
    assert_refused_and_nothing_left(
        &root,
        install(&ollama_spec(&root), &server.url("/garbage.tar.zst"), "v1"),
        "trailing garbage",
    );
}

// ---- paths that must not be trusted ----------------------------------------------------------------

/// The same hostile content in every format the installer reads, with the name each is served as.
fn hostile(entries: &[Entry]) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("h.tar.gz", tar_gz(entries)),
        ("h.tar.zst", tar_zst(entries)),
        // a zip has no hard links
        (
            "h.zip",
            zip(&entries
                .iter()
                .filter(|e| !matches!(e.kind, crate::testkit::archives::Kind::HardLink { .. }))
                .cloned()
                .collect::<Vec<_>>()),
        ),
    ]
}

#[test]
fn p3_z10a_nothing_is_ever_written_outside_the_install_directory() {
    // holds today and must go on holding: a path that climbs out is never followed
    let server = FileServer::start();
    for (name, bytes) in hostile(&[
        Entry::exe("bin/ollama", b"#!/bin/sh\n"),
        Entry::file("../escaped.txt", b"x"),
        Entry::file("lib/../../escaped2.txt", b"x"),
    ]) {
        if name.ends_with(".zst") {
            continue; // the format arrives with the work package
        }
        let root = scratch("z10a");
        server.add(&format!("/{name}"), bytes);
        let _ = install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1");
        for outside in [
            root.join("escaped.txt"),
            root.join("escaped2.txt"),
            root.parent().unwrap().join("escaped.txt"),
        ] {
            assert!(
                !outside.exists(),
                "{name}: {} was written",
                outside.display()
            );
        }
    }
}

#[test]
fn p3_z10_an_entry_that_climbs_out_or_is_absolute_refuses_the_whole_install() {
    let server = FileServer::start();
    for hostile_path in [
        "../escaped.txt",
        "lib/../../escaped.txt",
        "/tmp/quickai-escaped.txt",
    ] {
        for (name, bytes) in hostile(&[
            Entry::exe("bin/ollama", b"#!/bin/sh\n"),
            Entry::file(hostile_path, b"x"),
        ]) {
            let root = scratch("z10");
            server.add(&format!("/{name}"), bytes);
            let outcome = install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1");
            let message = outcome.expect_err(&format!("{name} with {hostile_path}: installed"));
            assert!(
                message.to_lowercase().contains("unsafe")
                    || message.to_lowercase().contains("outside"),
                "{name} with {hostile_path}: the message does not say why: {message}"
            );
            assert!(
                entries(&root).is_empty(),
                "{name}: left {:?}",
                entries(&root)
            );
            assert!(!root.join("escaped.txt").exists());
        }
    }
    assert!(!Path::new("/tmp/quickai-escaped.txt").exists());
}

#[cfg(unix)]
#[test]
fn p3_z11_a_link_that_points_out_and_a_file_written_through_it_are_refused() {
    let server = FileServer::start();
    // the same tree without the hostile entries installs in every format: it is the hostile
    // entries that are refused, not the format
    for (name, bytes) in hostile(&[
        Entry::exe("bin/ollama", b"#!/bin/sh\n"),
        Entry::symlink("lib/inside", "../bin/ollama"),
        Entry::file("lib/ok.txt", b"x"),
    ]) {
        let root = scratch("z11-control");
        server.add(&format!("/{name}"), bytes);
        install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1")
            .unwrap_or_else(|e| panic!("{name}: the harmless control did not install: {e}"));
    }
    for (name, bytes) in hostile(&[
        Entry::exe("bin/ollama", b"#!/bin/sh\n"),
        Entry::symlink("lib/out", "../.."),
        Entry::file("lib/out/pwned.txt", b"x"),
    ]) {
        let root = scratch("z11");
        server.add(&format!("/{name}"), bytes);
        let outcome = install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1");
        assert!(
            outcome.is_err(),
            "{name}: a link out of the tree was accepted"
        );
        assert!(
            !root.join("pwned.txt").exists(),
            "{name}: written through the link"
        );
        assert!(
            entries(&root).is_empty(),
            "{name}: left {:?}",
            entries(&root)
        );
    }
}

#[cfg(unix)]
#[test]
fn p3_z12_only_a_relative_link_that_stays_inside_the_tree_is_kept() {
    let server = FileServer::start();
    // the real archive's links are of this kind
    let root = scratch("z12-ok");
    server.add("/ok.tar.gz", tar_gz(&ollama_linux_tree()));
    install(&ollama_spec(&root), &server.url("/ok.tar.gz"), "v1")
        .expect("relative links inside are fine");
    assert_extracted(&root.join("v1"), &ollama_linux_tree());
    // a link to somewhere in the tree that does not exist yet (a later entry) is fine too
    let root = scratch("z12-forward");
    server.add(
        "/f.tar.gz",
        tar_gz(&[
            Entry::exe("bin/ollama", b"#!/bin/sh\n"),
            Entry::symlink("lib/a.so", "a.so.1"),
            Entry::file("lib/a.so.1", b"lib"),
        ]),
    );
    install(&ollama_spec(&root), &server.url("/f.tar.gz"), "v1")
        .expect("a link to a later entry is fine");

    for bad in ["/etc/passwd", "../../etc/passwd", "lib/../../../x"] {
        for (name, bytes) in hostile(&[
            Entry::exe("bin/ollama", b"#!/bin/sh\n"),
            Entry::symlink("lib/evil", bad),
        ]) {
            let root = scratch("z12-bad");
            server.add(&format!("/{name}"), bytes);
            let outcome = install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1");
            assert!(outcome.is_err(), "{name}: a link to {bad} was accepted");
            assert!(
                entries(&root).is_empty(),
                "{name}: left {:?}",
                entries(&root)
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn p3_z13_a_hard_link_is_refused() {
    let server = FileServer::start();
    for (name, bytes) in hostile(&[
        Entry::exe("bin/ollama", b"#!/bin/sh\n"),
        Entry::hardlink("lib/ollama-copy", "bin/ollama"),
    ]) {
        if name.ends_with(".zip") {
            continue;
        }
        let root = scratch("z13");
        server.add(&format!("/{name}"), bytes);
        let outcome = install(&ollama_spec(&root), &server.url(&format!("/{name}")), "v1");
        assert!(outcome.is_err(), "{name}: a hard link was accepted");
        assert!(
            entries(&root).is_empty(),
            "{name}: left {:?}",
            entries(&root)
        );
    }
}

// ---- a build is there completely or not at all -----------------------------------------------------

/// A tree whose middle is big enough that extracting it takes a moment: the binary first, 64 MB of
/// zeros (a few kilobytes gzipped), then a last small file.
fn big_tree() -> Vec<Entry> {
    vec![
        Entry::exe(
            &format!("bin/{}", crate::prims::install::binary_file_name("ollama")),
            b"#!/bin/sh\necho ollama version is 0.35.1\n",
        ),
        Entry::file("lib/ollama/big.bin", &vec![0u8; 64 * 1024 * 1024]),
        Entry::file("lib/ollama/last.txt", b"the last entry"),
    ]
}

fn complete(dir: &Path) -> bool {
    dir.join("lib/ollama/last.txt").is_file()
        && dir
            .join("bin")
            .join(crate::prims::install::binary_file_name("ollama"))
            .is_file()
}

#[test]
fn p3_z15_while_it_extracts_the_final_directory_never_holds_half_a_tree() {
    let root = scratch("z15");
    let server = FileServer::start();
    server.add("/big.tar.gz", tar_gz(&big_tree()));
    let label = "v1";
    let mut job = spawn_install(
        ollama_spec(&root),
        server.url("/big.tar.gz"),
        label.into(),
        || {},
    );

    let dir = root.join(label);
    let (mut samples_with_dir, mut saw_staging) = (0, false);
    let deadline = std::time::Instant::now() + Duration::from_secs(60);
    let outcome = loop {
        if dir.exists() {
            samples_with_dir += 1;
            assert!(
                complete(&dir),
                "the build directory existed with the install still under way (sample {samples_with_dir})"
            );
        }
        saw_staging |= root.join(format!("{label}.partial")).exists();
        if let Some(outcome) = job.poll() {
            break outcome;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the install never finished"
        );
        std::thread::sleep(Duration::from_millis(1));
    };
    outcome.expect("the install succeeded");
    assert!(complete(&dir));
    assert!(
        saw_staging,
        "the extraction never showed up as <label>.partial"
    );
    assert!(!root.join(format!("{label}.partial")).exists());
}

/// Runs in a process of its own (`spawn_test_child`): installs the archive named by the
/// environment and says so on stdout when it is done.
#[test]
#[ignore = "runs only as the child of p3_z16"]
fn child_main_install() {
    if !procs::is_test_child() {
        return;
    }
    let root = PathBuf::from(std::env::var("QUICKAI_P3_ROOT").unwrap());
    let url = std::env::var("QUICKAI_P3_URL").unwrap();
    let binary = std::env::var("QUICKAI_P3_BINARY").unwrap_or_else(|_| "ollama".to_string());
    let outcome = install(
        &spec(&root, &crate::prims::install::binary_file_name(&binary)),
        &url,
        "v1",
    );
    println!("install finished: {}", outcome.is_ok());
}

/// Kills an install of `archive` (served as `name`) at twelve moments, and after each looks at what
/// is listed as a build, then installs again: whatever the kill left, the next install works.
#[cfg(unix)]
fn killed_installs_leave_nothing_that_looks_installed(
    name: &str,
    archive: Vec<u8>,
    binary: &str,
    binary_in_build: &str,
    last_file: &str,
) {
    let server = FileServer::start();
    server.add(&format!("/{name}"), archive);
    let url = server.url(&format!("/{name}"));
    let whole = |dir: &Path| dir.join(last_file).is_file() && dir.join(binary_in_build).is_file();
    let mut killed_mid_way = 0;
    for delay_ms in [0u64, 4, 8, 14, 20, 30, 45, 60, 80, 110, 150, 200] {
        let root = scratch("z16");
        let mut child = procs::spawn_test_child(
            "p3::install::child_main_install",
            &[
                ("QUICKAI_P3_ROOT", root.to_str().unwrap()),
                ("QUICKAI_P3_URL", &url),
                ("QUICKAI_P3_BINARY", binary),
            ],
        );
        std::thread::sleep(Duration::from_millis(delay_ms));
        let _ = child.kill(); // SIGKILL: no chance to clean up
        let _ = child.wait();

        let spec = spec(&root, binary);
        let listed = list_installed(&spec);
        for build in &listed {
            assert!(
                whole(&build.install_dir),
                "killed after {delay_ms} ms: {} is listed as installed with files missing",
                build.install_dir.display()
            );
        }
        if listed.is_empty()
            && entries(&root)
                .iter()
                .any(|e| e.contains("partial") || e.contains("part"))
        {
            killed_mid_way += 1;
        }
        // whatever the kill left, the next install of the same build works
        let again = install(&spec, &url, "v1").expect("a fresh install after the kill");
        assert!(whole(&root.join("v1")), "{}", again.display());
        assert_eq!(
            entries(&root),
            ["v1"],
            "after the retry only the build is left"
        );
    }
    assert!(
        killed_mid_way > 0,
        "no kill landed while the install was under way: the test proved nothing"
    );
}

#[cfg(unix)]
#[test]
fn p3_z16_an_install_killed_at_any_moment_leaves_nothing_that_looks_installed() {
    killed_installs_leave_nothing_that_looks_installed(
        "big.tar.gz",
        tar_gz(&big_tree()),
        "ollama",
        "bin/ollama",
        "lib/ollama/last.txt",
    );
}

#[test]
fn p3_z17_the_leftovers_of_an_install_are_never_listed_as_builds() {
    let root = scratch("z17");
    let spec = ollama_spec(&root);
    // a finished build, and what a crashed install of another one leaves
    for dir in ["v1", "v2.partial", "v3.part"] {
        let bin = root.join(dir).join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        std::fs::write(bin.join(&spec.binary), b"#!/bin/sh\n").unwrap();
    }
    let labels: Vec<String> = list_installed(&spec).into_iter().map(|b| b.label).collect();
    assert_eq!(labels, ["v1"]);
}

#[cfg(unix)]
#[test]
fn p3_z18_a_stale_staging_directory_is_replaced_not_merged_by_the_next_install() {
    let root = scratch("z18");
    let stale = root.join("v1.partial");
    std::fs::create_dir_all(stale.join("lib")).unwrap();
    std::fs::write(stale.join("lib").join("old-junk.so"), b"from a crashed run").unwrap();
    let server = FileServer::start();
    server.add("/o.tar.gz", tar_gz(&ollama_linux_tree()));
    install(&ollama_spec(&root), &server.url("/o.tar.gz"), "v1").expect("installs");
    assert_extracted(&root.join("v1"), &ollama_linux_tree());
    assert!(
        !root.join("v1/lib/old-junk.so").exists(),
        "the stale files were merged into the build"
    );
    assert_eq!(entries(&root), ["v1"], "the staging directory was removed");
}

#[cfg(unix)]
#[test]
fn p3_z19_no_download_or_staging_files_remain_after_a_success_or_a_failed_download_or_extract() {
    let server = FileServer::start();
    server.add("/ok.tar.gz", tar_gz(&ollama_linux_tree()));
    server.add("/corrupt.tar.gz", b"this is not gzip at all".to_vec());
    let root = scratch("z19");
    install(&ollama_spec(&root), &server.url("/ok.tar.gz"), "v1").unwrap();
    assert_eq!(entries(&root), ["v1"]);
    for (path, label) in [("/corrupt.tar.gz", "bad1"), ("/missing.tar.gz", "bad3")] {
        assert!(install(&ollama_spec(&root), &server.url(path), label).is_err());
        assert_eq!(
            entries(&root),
            ["v1"],
            "after {path} the root holds more than the one good build"
        );
    }
}

#[test]
fn p3_z19b_an_archive_with_no_executable_leaves_nothing_behind() {
    // today the extracted files stay in a directory that is merely not listed
    let server = FileServer::start();
    server.add(
        "/no-binary.tar.gz",
        tar_gz(&[Entry::file("README", b"no executable in here")]),
    );
    let root = scratch("z19b");
    assert!(install(
        &ollama_spec(&root),
        &server.url("/no-binary.tar.gz"),
        "bad2"
    )
    .is_err());
    assert!(entries(&root).is_empty(), "left {:?}", entries(&root));
}

// ---- verified installs -------------------------------------------------------------------------------

/// A spec that checks digests, as the Ollama document asks.
#[cfg(unix)]
fn verifying_spec(root: &Path) -> InstallSpec {
    InstallSpec {
        verify: quickai_core::doc::engine::Verify::Digest,
        ..ollama_spec(root)
    }
}

#[cfg(unix)]
fn request(url: &str, host: Option<String>, pin: Option<String>) -> InstallRequest {
    InstallRequest {
        label: "v1".into(),
        base: Source {
            url: url.to_string(),
            host_digest: host,
            pin,
            size: None,
        },
        overlays: Vec::new(),
    }
}

/// What an install did: its outcome, what it said, how far it said it had got, and both in the
/// order they happened (`progress` stands for a progress report there).
#[cfg(unix)]
struct Ran {
    outcome: Result<PathBuf, String>,
    said: Vec<String>,
    progress: Vec<(u64, Option<u64>)>,
    order: Vec<String>,
}

/// Runs an install on this thread, keeping every event it sends.
#[cfg(unix)]
fn run(spec: &InstallSpec, request: &InstallRequest) -> Ran {
    let events = std::sync::Mutex::new((Vec::new(), Vec::new(), Vec::new()));
    let outcome = install_blocking(spec, request, &|event| {
        let mut events = events.lock().unwrap();
        match event {
            JobEvent::Status(line) => {
                events.2.push(line.clone());
                events.0.push(line);
            }
            JobEvent::Progress { downloaded, total } => {
                events.1.push((downloaded, total));
                events.2.push("progress".to_string());
            }
            _ => {}
        }
    });
    let (said, progress, order) = events.into_inner().unwrap();
    Ran {
        outcome,
        said,
        progress,
        order,
    }
}

#[cfg(unix)]
#[test]
fn p3_z30_a_download_with_the_digest_of_the_build_is_installed_and_one_that_differs_is_refused_before_it_is_unpacked(
) {
    let good = tar_gz(&linux_tree());
    let digest = sha256_hex(&good);
    let server = FileServer::start();
    server.add("/o.tar.gz", good.clone());

    let root = scratch("z30-ok");
    let Ran { outcome, said, .. } = run(
        &verifying_spec(&root),
        &request(
            &server.url("/o.tar.gz"),
            Some(format!("sha256:{digest}")),
            None,
        ),
    );
    outcome.expect("a download that has the digest the host lists installs");
    assert!(
        said.iter()
            .any(|l| l.contains("Checked") && l.contains(&digest[..12])),
        "the log does not say the file was checked: {said:?}"
    );

    // the same file, said to have another digest: the bytes are refused as damaged...
    let other = sha256_hex(b"not this file");
    let root = scratch("z30-bad");
    let Ran { outcome, .. } = run(
        &verifying_spec(&root),
        &request(&server.url("/o.tar.gz"), Some(other.clone()), None),
    );
    let message = outcome.expect_err("a file with another digest was installed");
    assert!(
        message.contains(&digest[..12]) && message.contains(&other[..12]),
        "the message names neither digest: {message}"
    );
    assert!(entries(&root).is_empty(), "left {:?}", entries(&root));

    // ...and that happens before extraction starts: an archive that could not be unpacked at all
    // (it is not gzip) is refused for its digest, not for what is inside
    server.add("/junk.tar.gz", b"this is not gzip at all".to_vec());
    let root = scratch("z30-first");
    let Ran { outcome, .. } = run(
        &verifying_spec(&root),
        &request(&server.url("/junk.tar.gz"), Some(other), None),
    );
    let message = outcome.unwrap_err();
    assert!(
        message.contains("checksum") && !message.contains("could not extract"),
        "the digest was not checked first: {message}"
    );
    assert!(entries(&root).is_empty());
}

#[cfg(unix)]
#[test]
fn p3_z31_a_pin_and_the_hosts_digest_must_agree_and_either_alone_is_enough() {
    let good = tar_gz(&linux_tree());
    let digest = sha256_hex(&good);
    let wrong = sha256_hex(b"something else");
    let server = FileServer::start();
    server.add("/o.tar.gz", good);
    let url = server.url("/o.tar.gz");

    // equal: installed; either one alone verifies
    for (host, pin) in [
        (Some(digest.clone()), Some(digest.clone())),
        (Some(digest.clone()), None),
        (None, Some(digest.clone())),
    ] {
        let root = scratch("z31-ok");
        let Ran { outcome, .. } = run(&verifying_spec(&root), &request(&url, host, pin));
        outcome.expect("a file with its digest is installed");
    }
    // a pin alone is held to as well
    let root = scratch("z31-pin");
    let Ran { outcome, .. } = run(
        &verifying_spec(&root),
        &request(&url, None, Some(wrong.clone())),
    );
    assert!(outcome.is_err());
    assert!(entries(&root).is_empty());

    // they differ: refused whichever is right, and without asking the server for the file
    for (host, pin) in [
        (digest.clone(), wrong.clone()),
        (wrong.clone(), digest.clone()),
    ] {
        let before = server.requests();
        let root = scratch("z31-differ");
        let Ran { outcome, .. } = run(
            &verifying_spec(&root),
            &request(&url, Some(host), Some(pin)),
        );
        let message = outcome.expect_err("digests that disagree were accepted");
        assert!(message.contains("changed after it was tested"), "{message}");
        assert_eq!(server.requests(), before, "the file was fetched anyway");
        assert!(entries(&root).is_empty());
    }
    // a digest that is not one is a mistake in the document, said at once
    let root = scratch("z31-garbage");
    let Ran { outcome, .. } = run(
        &verifying_spec(&root),
        &request(&url, Some("sha256:12345".into()), None),
    );
    assert!(outcome.unwrap_err().contains("not a sha256 digest"));
}

#[cfg(unix)]
#[test]
fn p3_z32_with_no_digest_anywhere_the_build_is_installed_and_the_log_says_it_was_not_checked() {
    let server = FileServer::start();
    server.add("/o.tar.gz", tar_gz(&linux_tree()));
    let root = scratch("z32");
    let Ran { outcome, said, .. } = run(
        &verifying_spec(&root),
        &request(&server.url("/o.tar.gz"), None, None),
    );
    outcome.expect("an address the person pasted is installed");
    assert!(said.iter().any(|l| l.contains("not verified")), "{said:?}");
    // a spec that does not verify (llama.cpp's) says nothing of the kind
    let root = scratch("z32-quiet");
    let Ran { outcome, said, .. } = run(
        &ollama_spec(&root),
        &request(&server.url("/o.tar.gz"), None, None),
    );
    outcome.unwrap();
    assert!(
        !said.iter().any(|l| l.contains("verified")),
        "a spec that does not verify mentioned verification: {said:?}"
    );
}

// ---- what a machine does not need -------------------------------------------------------------------

#[cfg(unix)]
fn skip_doc() -> &'static str {
    r#"schema = 1
kind = "engine"
id = "skipdemo"
requires = { primitives = ["spawn@1", "install.archive@1"] }
[engine]
name = "Demo"
[launch]
strategies = ["spawn"]
[launch.spawn]
program = "x"
[acquire]
root = "${data}/demo"
install = { use = "archive", binary = { name = "ollama" }, chmod = true, skip = [ { when = "gpu.known and not has(gpu.vendors, 'nvidia')", glob = "lib/ollama/cuda_*" } ] }
"#
}

#[cfg(unix)]
fn skip_spec(gpu_known: bool, vendors: &[&str], root: &Path) -> InstallSpec {
    use quickai_core::config::{load, Layer, LoadOptions, MemorySource};
    let source = MemorySource::new(&[("embedded:engines/skipdemo.toml", skip_doc())]);
    let registry = quickai_core::registry::Registry::build(
        load(&[(Layer::Embedded, &source)], &LoadOptions::default()),
        &crate::prims::known(),
    );
    assert!(
        registry.diagnostics.is_empty(),
        "{:#?}",
        registry.diagnostics
    );
    let facts = quickai_core::plan::HostFacts {
        gpu_known,
        gpu_vendors: vendors.iter().map(|v| v.to_string()).collect(),
        data_dir: root.display().to_string(),
        ..Default::default()
    };
    let mut spec = InstallSpec::from_engine(registry.engine("skipdemo").unwrap(), &facts)
        .expect("the demo engine installs");
    spec.root = root.to_path_buf();
    spec
}

#[cfg(unix)]
#[test]
fn p3_z33_the_cuda_libraries_are_left_out_on_a_machine_known_not_to_have_an_nvidia_gpu() {
    let server = FileServer::start();
    server.add("/o.tar.gz", tar_gz(&linux_tree()));
    let url = server.url("/o.tar.gz");

    // an AMD GPU is known and it is not NVIDIA: the CUDA directories are not unpacked
    let root = scratch("z33-amd");
    let spec = skip_spec(true, &["amd"], &root);
    assert_eq!(spec.skip, ["lib/ollama/cuda_*"]);
    install(&spec, &url, "v1").expect("installs");
    let build = root.join("v1");
    assert!(build.join("bin/ollama").is_file());
    assert!(build.join("lib/ollama/vulkan/libggml-vulkan.so").is_file());
    assert!(build.join("lib/ollama/libggml-base.so.0.25.3").is_file());
    for left_out in [
        "lib/ollama/cuda_v12",
        "lib/ollama/cuda_v13",
        "lib/ollama/cuda_v12/libggml-cuda.so",
        "lib/ollama/cuda_v12/libcudart.so.12",
    ] {
        assert!(
            std::fs::symlink_metadata(build.join(left_out)).is_err(),
            "{left_out} was unpacked on a machine with no NVIDIA GPU"
        );
    }
    let skipped_size = list_installed(&spec)[0].size_bytes;

    // an NVIDIA GPU, or none detected: nothing is left out
    for (known, vendors, name) in [(true, vec!["nvidia"], "nvidia"), (false, vec![], "unknown")] {
        let root = scratch("z33-all");
        let spec = skip_spec(known, &vendors, &root);
        assert!(spec.skip.is_empty(), "{name}: {:?}", spec.skip);
        install(&spec, &url, "v1").expect("installs");
        assert_extracted(&root.join("v1"), &linux_tree());
        assert!(
            list_installed(&spec)[0].size_bytes > skipped_size + 40_000,
            "{name}: the size does not show that the CUDA libraries came"
        );
    }
}

// ---- the program must run ---------------------------------------------------------------------------

#[cfg(unix)]
fn smoking_spec(root: &Path, millis: u64) -> InstallSpec {
    InstallSpec {
        smoke: Some(Smoke {
            args: vec!["--version".into()],
            timeout: Duration::from_millis(millis),
        }),
        ..ollama_spec(root)
    }
}

#[cfg(unix)]
fn tree_with_binary(script: &[u8]) -> Vec<Entry> {
    let mut tree = linux_tree();
    tree[0] = Entry::exe("bin/ollama", script);
    tree
}

#[cfg(unix)]
#[test]
fn p3_z34_the_unpacked_program_must_run_before_the_build_counts_as_installed() {
    let server = FileServer::start();
    server.add("/good.tar.gz", tar_gz(&linux_tree()));
    server.add(
        "/broken.tar.gz",
        tar_gz(&tree_with_binary(
            b"#!/bin/sh\necho 'cannot load libggml: wrong ELF class' >&2\nexit 3\n",
        )),
    );
    server.add(
        "/hangs.tar.gz",
        tar_gz(&tree_with_binary(b"#!/bin/sh\nexec sleep 30\n")),
    );

    let root = scratch("z34-ok");
    let Ran { outcome, said, .. } = run(
        &smoking_spec(&root, 5_000),
        &request(&server.url("/good.tar.gz"), None, None),
    );
    outcome.expect("a program that runs and exits 0 is installed");
    assert!(said.iter().any(|l| l.contains("Checking")), "{said:?}");

    // a program that fails: refused with what it said, and nothing is installed
    let root = scratch("z34-fails");
    let Ran { outcome, .. } = run(
        &smoking_spec(&root, 5_000),
        &request(&server.url("/broken.tar.gz"), None, None),
    );
    let message = outcome.expect_err("a program that exits 3 was installed");
    assert!(
        message.contains("wrong ELF class") && message.contains("exit"),
        "the program's own words are not in the message: {message}"
    );
    assert!(entries(&root).is_empty(), "left {:?}", entries(&root));

    // a program that never answers is killed at the timeout and refused
    let root = scratch("z34-hangs");
    let started = std::time::Instant::now();
    let Ran { outcome, .. } = run(
        &smoking_spec(&root, 400),
        &request(&server.url("/hangs.tar.gz"), None, None),
    );
    let message = outcome.expect_err("a program that hangs was installed");
    assert!(message.contains("did not finish"), "{message}");
    assert!(
        started.elapsed() < Duration::from_secs(10),
        "the timeout was not honoured: {:?}",
        started.elapsed()
    );
    assert!(entries(&root).is_empty());
}

// ---- room --------------------------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_z35_an_install_that_will_not_fit_is_refused_before_anything_is_requested() {
    let server = FileServer::start();
    server.add("/o.tar.gz", tar_gz(&linux_tree()));
    let mut ask = request(&server.url("/o.tar.gz"), None, None);
    ask.base.size = Some(1_440_000_000);

    let root = scratch("z35-full");
    let tight = InstallSpec {
        free_space: |_| Some(2_000_000_000),
        ..ollama_spec(&root)
    };
    let Ran { outcome, .. } = run(&tight, &ask);
    let message = outcome.expect_err("an install that cannot fit was started");
    assert!(
        message.contains("not enough free space")
            && message.contains("needed")
            && message.contains("free"),
        "{message}"
    );
    assert_eq!(server.requests(), 0, "the server was asked anyway");
    assert!(entries(&root).is_empty());

    // enough room: it goes ahead; a size nobody knows is not a reason to refuse
    let roomy_root = scratch("z35-roomy");
    let roomy = InstallSpec {
        free_space: |_| Some(100_000_000_000),
        ..ollama_spec(&roomy_root)
    };
    // (the size the host lists is not the size of this small test file; only the room is checked)
    ask.base.size = Some(1_440_000_000);
    let Ran { outcome, .. } = run(&roomy, &ask);
    outcome.expect("an install with room goes ahead");
    let unknown_root = scratch("z35-unknown");
    let unknown = InstallSpec {
        free_space: |_| Some(10),
        ..ollama_spec(&unknown_root)
    };
    let Ran { outcome, .. } = run(&unknown, &request(&server.url("/o.tar.gz"), None, None));
    outcome.expect("with no size listed there is nothing to compare, and it goes ahead");
}

// ---- overlays and progress ----------------------------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_z36_an_overlay_is_unpacked_over_the_same_build_and_a_failing_one_leaves_nothing() {
    let server = FileServer::start();
    server.add("/base.tar.gz", tar_gz(&linux_tree()));
    let rocm = vec![
        Entry::exe("lib/ollama/rocm/libggml-hip.so", &pseudo(20, 9_000)),
        // an overlay may replace a file of the base
        Entry::file("lib/ollama/GO_LICENSE", b"replaced by the overlay\n"),
    ];
    server.add("/rocm.tar.gz", tar_gz(&rocm));
    server.add("/broken.tar.gz", b"not an archive".to_vec());

    let root = scratch("z36");
    let mut ask = request(&server.url("/base.tar.gz"), None, None);
    ask.overlays.push(Source::bare(&server.url("/rocm.tar.gz")));
    let Ran { outcome, .. } = run(&ollama_spec(&root), &ask);
    outcome.expect("base and overlay install");
    let build = root.join("v1");
    assert!(build.join("lib/ollama/vulkan/libggml-vulkan.so").is_file());
    assert!(build.join("lib/ollama/rocm/libggml-hip.so").is_file());
    assert_eq!(
        std::fs::read(build.join("lib/ollama/GO_LICENSE")).unwrap(),
        b"replaced by the overlay\n"
    );
    assert_eq!(entries(&root), ["v1"], "one build, renamed once");

    // an overlay that fails takes the whole install with it
    let root = scratch("z36-broken");
    let mut ask = request(&server.url("/base.tar.gz"), None, None);
    ask.overlays
        .push(Source::bare(&server.url("/broken.tar.gz")));
    let Ran { outcome, .. } = run(&ollama_spec(&root), &ask);
    assert!(outcome.is_err());
    assert!(entries(&root).is_empty(), "left {:?}", entries(&root));

    // an overlay nobody chose is never fetched
    let before = server.paths_asked().len();
    let root = scratch("z36-none");
    let Ran { outcome, .. } = run(
        &ollama_spec(&root),
        &request(&server.url("/base.tar.gz"), None, None),
    );
    outcome.unwrap();
    let asked: Vec<String> = server.paths_asked().into_iter().skip(before).collect();
    assert_eq!(asked, ["/base.tar.gz"], "something else was fetched");
}

#[cfg(unix)]
#[test]
fn p3_z37_progress_only_grows_ends_at_the_total_and_unpacking_is_its_own_step() {
    let mut tree = linux_tree();
    tree.push(Entry::file(
        "lib/ollama/filler.bin",
        &pseudo(30, 3 * 1024 * 1024),
    ));
    let archive = tar_gz(&tree);
    let size = archive.len() as u64;
    let server = FileServer::start();
    server.add("/o.tar.gz", archive);

    let root = scratch("z37");
    let mut ask = request(&server.url("/o.tar.gz"), None, None);
    ask.base.size = Some(size);
    let Ran {
        outcome,
        said,
        progress,
        order,
    } = run(&verifying_spec(&root), &ask);
    outcome.expect("installs");
    assert!(progress.len() >= 2, "{progress:?}");
    assert!(
        progress.windows(2).all(|w| w[0].0 <= w[1].0),
        "progress went backwards: {progress:?}"
    );
    assert_eq!(
        *progress.last().unwrap(),
        (size, Some(size)),
        "it does not end at the total the host listed"
    );
    assert!(
        progress.iter().all(|(_, total)| *total == Some(size)),
        "the total moved while it downloaded: {progress:?}"
    );
    assert!(said.iter().any(|l| l.starts_with("Unpacking")), "{said:?}");
    // the step comes when the download is over, not before it
    let unpacking = order
        .iter()
        .position(|l| l.starts_with("Unpacking"))
        .expect("unpacking is reported as a step of its own");
    let last_progress = order.iter().rposition(|l| l == "progress").unwrap();
    assert!(
        last_progress < unpacking,
        "unpacking was announced before the download had finished: {order:?}"
    );
}

// ---- llama.cpp goes through the same pipeline ---------------------------------------------------------

#[cfg(unix)]
#[test]
fn p3_z38_a_llamacpp_install_killed_midway_leaves_nothing_listed_either() {
    // the shape of a llama.cpp release: a flat zip with its server and libraries
    let tree = vec![
        Entry::exe("llama-server", b"#!/bin/sh\necho server\n"),
        Entry::file("libggml.so", b"lib"),
        Entry::file("big.bin", &vec![0u8; 64 * 1024 * 1024]),
        Entry::file("last.txt", b"the last entry"),
    ];
    killed_installs_leave_nothing_that_looks_installed(
        "llama.zip",
        zip(&tree),
        "llama-server",
        "llama-server",
        "last.txt",
    );
}

// ---- the real archive (by hand) -------------------------------------------------------------------------

/// The sha256 the document pins for `ollama-linux-amd64.tar.zst` of v0.35.1 (engine-facts.md, O17).
#[cfg(unix)]
const REAL_LINUX_PIN: &str = "9fcd79ac4575b2bd31b992eee18b1000c8ad126b451627c8f8cd091714cfbb10";

/// Installs the real 1.44 GB archive the way the application will: through the server, with its
/// digest checked against the pin, the CUDA libraries left out (102 MB instead of 2.2 GB), the real
/// binary run as the smoke test; then again with everything, to see how long the 2.2 GB take.
#[cfg(unix)]
#[test]
#[ignore = "manual: needs the real ollama-linux-amd64.tar.zst (QUICKAI_REAL_OLLAMA_ARCHIVE=<path>)"]
fn p3_z39_the_real_ollama_archive_installs_through_the_pipeline() {
    let path = std::env::var_os("QUICKAI_REAL_OLLAMA_ARCHIVE")
        .expect("set QUICKAI_REAL_OLLAMA_ARCHIVE to ollama-linux-amd64.tar.zst");
    let bytes = std::fs::read(path).expect("read the archive");
    assert_eq!(
        sha256_hex(&bytes),
        REAL_LINUX_PIN,
        "this is not the pinned release"
    );
    let size = bytes.len() as u64;
    let server = FileServer::start();
    server.add("/ollama-linux-amd64.tar.zst", bytes);
    let mut ask = request(
        &server.url("/ollama-linux-amd64.tar.zst"),
        Some(format!("sha256:{REAL_LINUX_PIN}")),
        Some(REAL_LINUX_PIN.to_string()),
    );
    ask.base.size = Some(size);

    for (skip, expect_at_most, name) in [
        (
            vec!["lib/ollama/cuda_*".to_string()],
            200_000_000u64,
            "without CUDA",
        ),
        (vec![], u64::MAX, "everything"),
    ] {
        let root = scratch("z39");
        let spec = InstallSpec {
            skip,
            smoke: Some(Smoke {
                args: vec!["--version".into()],
                timeout: Duration::from_secs(30),
            }),
            ..verifying_spec(&root)
        };
        let started = std::time::Instant::now();
        let Ran { outcome, said, .. } = run(&spec, &ask);
        let took = started.elapsed();
        let binary = outcome.unwrap_or_else(|e| panic!("{name}: {e}"));
        let installed = list_installed(&spec);
        println!(
            "{name}: installed in {took:?}, {} bytes on disk; said {said:?}",
            installed[0].size_bytes
        );
        assert!(installed[0].size_bytes <= expect_at_most, "{name}: too big");
        let out = std::process::Command::new(&binary)
            .arg("--version")
            .output()
            .unwrap();
        assert!(
            String::from_utf8_lossy(&out.stdout).contains("0.35.1")
                || String::from_utf8_lossy(&out.stderr).contains("0.35.1"),
            "the installed binary does not report v0.35.1"
        );
        assert_eq!(entries(&root), ["v1"]);
        assert!(took < Duration::from_secs(300), "{name}: took {took:?}");
    }
}
