//! Golden notebooks written by nbformat stay byte-identical on save.

use std::path::PathBuf;

use europa_core::{parse, serialize};

fn golden_dir(kind: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("golden")
        .join(kind)
}

fn notebooks(kind: &str) -> Vec<(String, Vec<u8>)> {
    let mut found: Vec<(String, Vec<u8>)> = std::fs::read_dir(golden_dir(kind))
        .unwrap_or_else(|error| panic!("golden/{kind} is readable: {error}"))
        .map(|entry| entry.expect("directory entry").path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "ipynb"))
        .map(|path| {
            let name = path
                .file_name()
                .expect("file name")
                .to_string_lossy()
                .into_owned();
            (name, std::fs::read(&path).expect("fixture reads"))
        })
        .collect();
    found.sort();
    found
}

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
                    "line {number}: expected {:?}, got {:?}",
                    left.unwrap_or("<end>"),
                    right.unwrap_or("<end>")
                )
            }
        }
    }
}

#[test]
fn every_foreign_source_has_a_jupyter_counterpart() {
    let foreign = notebooks("foreign");
    let jupyter = notebooks("jupyter");
    assert!(!jupyter.is_empty(), "golden/jupyter must not be empty");
    let names = |list: &[(String, Vec<u8>)]| {
        list.iter()
            .map(|(name, _)| name.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(names(&foreign), names(&jupyter));
}

#[test]
fn jupyter_files_are_fixed_points_of_open_and_save() {
    let mut failures = Vec::new();
    let mut saved_all = Vec::new();
    for (name, bytes) in notebooks("jupyter") {
        let notebook = parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"));
        let saved = serialize(&notebook);
        let original = String::from_utf8(bytes).expect("fixture is UTF-8");
        if saved != original {
            failures.push(format!("{name}: {}", first_difference(&original, &saved)));
        }
        saved_all.push((name, saved));
    }
    assert!(
        failures.is_empty(),
        "not byte-stable:\n{}",
        failures.join("\n")
    );
    for (name, saved) in saved_all {
        insta::assert_snapshot!(format!("golden-jupyter-{name}"), saved);
    }
}
