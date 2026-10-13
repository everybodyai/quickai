//! The bridge that gives ComfyUI's page a "Download" that installs (`assets/comfyui/quickai_bridge`,
//! docs/comfyui.md): its downloads are Python, tested in `tests/comfy_bridge/test_fetch.py` against a server
//! that behaves like the hub, and each of those tests is a row of docs/phase3-test-plan.md (group P). The
//! Python test of a row is run here, by the Python of the machine, so that `cargo test` holds the rows to
//! what the plan says like any others.
//!
//! The Python is the first of `QUICKAI_TEST_PYTHON`, `python3` and `python` that is 3.10 or newer, which is
//! what the install of ComfyUI asks for too; a machine that has none fails these tests and says so.

use std::path::Path;
use std::process::Command;

fn python() -> String {
    let wanted = "import sys; sys.exit(0 if sys.version_info >= (3, 10) else 1)";
    let named = std::env::var("QUICKAI_TEST_PYTHON").ok();
    for candidate in named
        .iter()
        .map(String::as_str)
        .chain(["python3", "python"])
    {
        let runs = Command::new(candidate)
            .args(["-I", "-c", wanted])
            .output()
            .is_ok_and(|o| o.status.success());
        if runs {
            return candidate.to_string();
        }
    }
    panic!(
        "the bridge's tests need a Python of 3.10 or newer (python3 or python on the PATH, or QUICKAI_TEST_PYTHON)"
    );
}

/// Runs the one Python test whose name begins `test_<row>_`, and holds it to having run.
fn run(row: &str) {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/comfy_bridge");
    let output = Command::new(python())
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .args([
            "-I",
            "-W",
            "error::ResourceWarning",
            "-m",
            "unittest",
            "discover",
        ])
        .arg("-s")
        .arg(&dir)
        .args(["-p", "test_*.py", "-k"])
        .arg(format!("test_{row}_"))
        .output()
        .expect("python could not be started");
    let text = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success() && text.contains("Ran 1 test in") && text.contains("OK"),
        "the Python test of row {row} did not pass (exit {:?}):\n{text}\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn p3_p01_a_file_is_fetched_into_the_folder_of_its_kind_whole_and_checked() {
    run("p01");
}

#[test]
fn p3_p02_what_is_not_the_hubs_or_not_a_format_of_numbers_or_not_plain_is_refused() {
    run("p02");
}

#[test]
fn p3_p03_a_part_that_a_stop_left_is_gone_on_from_and_the_whole_is_checked() {
    run("p03");
}

#[test]
fn p3_p04_a_hub_that_ignores_the_range_or_says_the_part_is_all_there_is_is_met() {
    run("p04");
}

#[test]
fn p3_p05_a_file_that_is_not_what_the_hub_describes_is_not_kept() {
    run("p05");
}

#[test]
fn p3_p06_a_fetch_that_stops_short_keeps_what_came_and_asking_again_finishes_it() {
    run("p06");
}

#[test]
fn p3_p07_a_fetch_the_person_stops_leaves_nothing_and_the_next_one_goes_on() {
    run("p07");
}

#[test]
fn p3_p08_a_file_that_is_there_is_not_fetched_again_and_the_same_ask_is_one_download() {
    run("p08");
}

#[test]
fn p3_p09_a_hub_that_says_no_is_told_to_the_person_and_the_page_may_leave_it_to_the_browser() {
    run("p09");
}

#[test]
fn p3_p10_a_file_that_does_not_fit_on_the_disk_is_not_begun() {
    run("p10");
}

#[test]
fn p3_p11_the_folder_is_the_kinds_own_and_a_name_with_a_folder_in_it_keeps_it() {
    run("p11");
}

#[test]
fn p3_p12_the_routes_answer_this_machine_only_and_the_setting_leaves_the_button_to_the_browser() {
    run("p12");
}

/// Row P19, by hand: the page of a real ComfyUI in a real browser (`tests/comfy_bridge/real_page.py`, whose
/// header says what it holds to). It needs a build of ComfyUI that QuickAI installed (`QUICKAI_P19_BUILD`), a
/// Python that has Playwright and its Chromium (`QUICKAI_P19_PYTHON`, else the Python of the other tests), the
/// internet (350 MB from huggingface.co) and 700 MB of room for its work folder (`QUICKAI_P19_WORK`, for a
/// machine whose temporary folder is memory).
#[test]
#[ignore = "manual: needs a ComfyUI build (QUICKAI_P19_BUILD), Python with playwright and Chromium, and the internet (350 MB from huggingface.co)"]
fn p3_p19_the_real_page_installs_a_model_goes_on_after_a_kill_and_leaves_the_rest_to_the_browser() {
    let build = std::env::var("QUICKAI_P19_BUILD")
        .expect("QUICKAI_P19_BUILD=<a build of ComfyUI that QuickAI installed>");
    let python = std::env::var("QUICKAI_P19_PYTHON").unwrap_or_else(|_| python());
    let script = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/comfy_bridge/real_page.py");
    let mut command = Command::new(python);
    command
        .env("PYTHONDONTWRITEBYTECODE", "1")
        .arg(script)
        .arg(build);
    if let Ok(work) = std::env::var("QUICKAI_P19_WORK") {
        command.arg(work);
    }
    let status = command.status().expect("python could not be started");
    assert!(status.success(), "the real page did not do what it should");
}
