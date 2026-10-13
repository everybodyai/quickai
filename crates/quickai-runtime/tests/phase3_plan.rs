//! Keeps `docs/phase3-test-plan.md` and the Phase 3 tests in step, in both directions.
//!
//! A row of the plan with a state of `ready`, `red` or `manual` has a test named `p3_<id>_...`
//! somewhere in the workspace; a row that is `spec` has none yet; every such test is a row. A `ready`
//! test is not ignored, a `red` one is ignored for a reason that starts with "phase 3", a `manual`
//! one for a reason that starts with "manual". The counts the plan states are the counts of its rows.
//! A plan that says one thing and a test tree another is how a gate gets signed off on nothing.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("..")
}

fn is_id(cell: &str) -> bool {
    let b = cell.as_bytes();
    b.len() >= 3
        && b[0].is_ascii_uppercase()
        && b[1].is_ascii_digit()
        && b[2].is_ascii_digit()
        && (b.len() == 3 || (b.len() == 4 && b[3].is_ascii_lowercase()))
}

/// `(id as written, state)` of every case row.
fn plan_rows() -> Vec<(String, String)> {
    let text = std::fs::read_to_string(repo().join("docs/phase3-test-plan.md")).expect("the plan");
    text.lines()
        .filter(|l| l.starts_with("| "))
        .filter_map(|l| {
            let cells: Vec<&str> = l.split('|').map(str::trim).collect();
            let (id, state) = (cells.get(1)?, cells.get(3)?);
            (is_id(id) && ["ready", "red", "manual", "spec"].contains(state))
                .then(|| (id.to_string(), state.to_string()))
        })
        .collect()
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if path.is_dir() {
            if name != "target" && name != "fixtures" {
                rust_files(&path, out);
            }
        } else if name.ends_with(".rs") && name != "phase3_plan.rs" {
            out.push(path);
        }
    }
}

/// `id (lower case, as in the name) -> ignore reason` of every `fn p3_<id>_...` in the workspace.
fn tests_in_code() -> BTreeMap<String, Option<String>> {
    let mut files = Vec::new();
    rust_files(&repo().join("crates"), &mut files);
    let mut found = BTreeMap::new();
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        for (i, line) in lines.iter().enumerate() {
            let Some(rest) = line.trim_start().strip_prefix("fn p3_") else {
                continue;
            };
            let id: String = rest.chars().take_while(|c| *c != '_').collect();
            if !is_id(
                &id.to_ascii_uppercase()
                    .replacen(|c: char| c.is_ascii_lowercase(), "", 0),
            ) && id.len() < 3
            {
                continue;
            }
            // the attributes above the function
            let mut reason = None;
            let mut j = i;
            while j > 0 {
                j -= 1;
                let above = lines[j].trim();
                if !(above.starts_with("#[") || above.starts_with("//")) {
                    break;
                }
                if let Some(r) = above.strip_prefix("#[ignore = \"") {
                    reason = Some(r.trim_end_matches("\"]").to_string());
                }
            }
            let previous = found.insert(id.clone(), reason);
            assert!(previous.is_none(), "two tests are named p3_{id}_...");
        }
    }
    found
}

#[test]
fn the_plan_and_the_tests_agree() {
    let rows = plan_rows();
    assert!(rows.len() > 150, "the plan lost its rows: {}", rows.len());
    let code = tests_in_code();

    let mut seen = BTreeMap::new();
    for (id, state) in &rows {
        assert!(
            seen.insert(id.to_ascii_lowercase(), state.clone())
                .is_none(),
            "{id} is in the plan twice"
        );
    }
    let mut problems = Vec::new();
    for (id, state) in &seen {
        match (state.as_str(), code.get(id)) {
            ("spec", Some(_)) => problems.push(format!(
                "{id} is `spec` in the plan but has a test: make it `red` or `ready`"
            )),
            ("spec", None) => {}
            (_, None) => problems.push(format!("{id} is `{state}` in the plan but has no test")),
            ("ready", Some(Some(reason))) => {
                problems.push(format!("{id} is `ready` but is ignored: {reason}"))
            }
            ("ready", Some(None)) => {}
            ("red", Some(Some(r))) if r.starts_with("phase 3") => {}
            ("red", Some(other)) => problems.push(format!(
                "{id} is `red` but its ignore says {other:?}, not \"phase 3 ...\""
            )),
            ("manual", Some(Some(r))) if r.starts_with("manual") => {}
            ("manual", Some(other)) => {
                problems.push(format!("{id} is `manual` but its ignore says {other:?}"))
            }
            (s, _) => problems.push(format!("{id}: unknown state {s}")),
        }
    }
    for id in code.keys() {
        if !seen.contains_key(id) {
            problems.push(format!("the test p3_{id}_... is not a row of the plan"));
        }
    }
    assert!(problems.is_empty(), "{}", problems.join("\n"));
}

#[test]
fn the_counts_the_plan_states_are_the_counts_of_its_rows() {
    let text = std::fs::read_to_string(repo().join("docs/phase3-test-plan.md")).unwrap();
    let line = text
        .lines()
        .find(|l| l.starts_with("**Counts**"))
        .expect("a **Counts** line");
    // "**Counts** (`ready` / `red` / `manual` / `spec`): 19 / 112 / 1 / 54 (total 186). The test kit ..."
    let stated_part = line
        .split("`spec`):")
        .nth(1)
        .expect("the counts follow `spec`):");
    let stated_part = stated_part.split(')').next().unwrap();
    let numbers: Vec<usize> = stated_part
        .split(|c: char| !c.is_ascii_digit())
        .filter(|s| !s.is_empty())
        .map(|s| s.parse().unwrap())
        .collect();
    let rows = plan_rows();
    let count = |state: &str| rows.iter().filter(|(_, s)| s == state).count();
    // "(ready / red / manual / spec): 19 / 112 / 1 / 54 (total 186)"
    let stated = &numbers[numbers.len() - 5..];
    assert_eq!(
        stated,
        [
            count("ready"),
            count("red"),
            count("manual"),
            count("spec"),
            rows.len()
        ],
        "the counts line of the plan is out of date: {line}"
    );
}
