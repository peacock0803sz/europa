//! Writing back the acceptance corpus matches nbformat, run ignored with the
//! corpus rewritten by `scripts/nbformat-rewrite.sh corpus`.

use std::path::PathBuf;

use europa_core::{parse, serialize};

/// Describe the first line where two texts differ.
fn first_difference(expected: &str, actual: &str) -> String {
    let mut expected_lines = expected.split('\n');
    let mut actual_lines = actual.split('\n');
    let mut number = 1;
    loop {
        match (expected_lines.next(), actual_lines.next()) {
            (Some(left), Some(right)) if left == right => number += 1,
            (None, None) => return "no difference".to_owned(),
            (left, right) => {
                return format!(
                    "line {number}: nbformat wrote {:?}, Europa wrote {:?}",
                    left.unwrap_or("<end>"),
                    right.unwrap_or("<end>")
                )
            }
        }
    }
}

#[test]
#[ignore]
fn rewrite_matches_jupyter() {
    let dir = std::env::var("EUROPA_JUPYTER_DIR").unwrap_or_default();
    assert!(
        !dir.trim().is_empty(),
        "EUROPA_JUPYTER_DIR must point at the nbformat rewrite of the corpus; \
         run bash scripts/nbformat-rewrite.sh corpus"
    );
    let root = {
        let candidate = PathBuf::from(&dir);
        if candidate.is_absolute() {
            candidate
        } else {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../..")
                .join(candidate)
        }
    };
    assert!(root.is_dir(), "EUROPA_JUPYTER_DIR does not exist: {dir}");

    let expected_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus-expected.tsv");
    let expected_text =
        std::fs::read_to_string(&expected_path).expect("corpus-expected.tsv exists");
    let loadable: Vec<&str> = expected_text
        .lines()
        .filter_map(|line| line.split_once('\t'))
        .filter(|(_, verdict)| *verdict == "load")
        .map(|(rel, _)| rel)
        .collect();
    assert!(!loadable.is_empty(), "no loadable corpus entries");

    let mut failures = Vec::new();
    for rel in &loadable {
        let Ok(rewritten) = std::fs::read(root.join(rel)) else {
            failures.push(format!("{rel}: no nbformat rewrite found"));
            continue;
        };
        let notebook = match parse(&rewritten) {
            Ok(notebook) => notebook,
            Err(error) => {
                failures.push(format!("{rel}: Europa cannot read the rewrite: {error}"));
                continue;
            }
        };
        let saved = serialize(&notebook);
        let expected = String::from_utf8_lossy(&rewritten);
        if saved != expected {
            failures.push(format!("{rel}: {}", first_difference(&expected, &saved)));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} files differ:\n{}",
        failures.len(),
        loadable.len(),
        failures.join("\n")
    );
}
