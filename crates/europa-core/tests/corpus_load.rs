//! Acceptance corpus loading test, run ignored with a fetched corpus.

use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Instant;

use europa_core::{parse, Cell, ParseError};

fn cell_kind(cell: &Cell) -> &'static str {
    match cell {
        Cell::Code(_) => "code",
        Cell::Markdown(_) => "markdown",
        Cell::Raw(_) => "raw",
        Cell::Unknown(_) => "unknown",
    }
}

#[test]
#[ignore]
fn load_acceptance_corpus() {
    let dir = std::env::var("EUROPA_CORPUS_DIR").expect(
        "EUROPA_CORPUS_DIR must point at the fetched corpus; run bash scripts/fetch-corpus.sh",
    );
    assert!(
        !dir.trim().is_empty(),
        "EUROPA_CORPUS_DIR must not be empty"
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
    assert!(root.is_dir(), "EUROPA_CORPUS_DIR does not exist: {dir}");

    let expected_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("corpus-expected.tsv");
    let expected_text = std::fs::read_to_string(&expected_path)
        .expect("corpus-expected.tsv must exist; run bash scripts/classify-corpus.sh");
    let mut expected: HashMap<String, String> = HashMap::new();
    for (line_no, line) in expected_text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let (path, verdict) = line
            .split_once('\t')
            .unwrap_or_else(|| panic!("malformed TSV line {}", line_no + 1));
        assert!(
            !matches!(verdict, "review"),
            "review remains for {path}; resolve it in corpus-overrides.tsv"
        );
        expected.insert(path.to_owned(), verdict.to_owned());
    }

    let mut found: Vec<String> = Vec::new();
    let mut stack: Vec<PathBuf> = vec![root.clone()];
    while let Some(current) = stack.pop() {
        for entry in std::fs::read_dir(&current).expect("read corpus directory") {
            let entry = entry.expect("corpus entry");
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("ipynb") {
                let rel = path
                    .strip_prefix(&root)
                    .expect("relative corpus path")
                    .to_string_lossy()
                    .replace('\\', "/");
                found.push(rel);
            }
        }
    }
    found.sort();

    for rel in &found {
        assert!(
            expected.contains_key(rel),
            "corpus file without expectation: {rel}"
        );
    }

    for (rel, verdict) in &expected {
        let full = root.join(rel);
        assert!(full.is_file(), "expected file missing from corpus: {rel}");
        let bytes = std::fs::read(&full).expect("read notebook file");
        let started = Instant::now();
        let parsed = parse(&bytes);
        let elapsed = started.elapsed();
        match verdict.as_str() {
            "load" => {
                assert!(
                    elapsed.as_secs_f32() < 5.0,
                    "loading {rel} took too long: {elapsed:?}"
                );
                let notebook =
                    parsed.unwrap_or_else(|error| panic!("expected {rel} to load, got {error:?}"));
                let raw: serde_json::Value =
                    serde_json::from_slice(&bytes).expect("expected file parses as JSON");
                let raw_cells = raw
                    .get("cells")
                    .and_then(|cells| cells.as_array())
                    .expect("expected cells array");
                assert_eq!(
                    notebook.cells.len(),
                    raw_cells.len(),
                    "cell count for {rel}"
                );
                let raw_kinds: Vec<&str> = raw_cells
                    .iter()
                    .map(|cell| {
                        cell.get("cell_type")
                            .and_then(|kind| kind.as_str())
                            .expect("cell_type string")
                    })
                    .collect();
                for (idx, (cell, raw_kind)) in
                    notebook.cells.iter().zip(raw_kinds.iter()).enumerate()
                {
                    let kind = cell_kind(cell);
                    if matches!(raw_kind, &"code" | &"markdown" | &"raw") {
                        assert_eq!(kind, *raw_kind, "cell kind at {rel} index {idx}");
                    } else {
                        assert_eq!(kind, "unknown", "unknown kind at {rel} index {idx}");
                    }
                }
                for (idx, (cell, raw_cell)) in
                    notebook.cells.iter().zip(raw_cells.iter()).enumerate()
                {
                    let raw_outputs = raw_cell
                        .get("outputs")
                        .and_then(|outputs| outputs.as_array());
                    match cell {
                        Cell::Code(code) => {
                            let expected_len = raw_outputs.map_or(0, Vec::len);
                            assert_eq!(
                                code.outputs.len(),
                                expected_len,
                                "outputs length at {rel} cell {idx}"
                            );
                        }
                        _ => assert!(
                            raw_outputs.is_none(),
                            "non-code outputs at {rel} cell {idx}"
                        ),
                    }
                }
            }
            "a" => assert!(
                matches!(parsed, Err(ParseError::InvalidJson { .. })),
                "expected (a) for {rel}, got {parsed:?}"
            ),
            "b" => assert!(
                matches!(parsed, Err(ParseError::InvalidNbformat { .. })),
                "expected (b) for {rel}, got {parsed:?}"
            ),
            "c" => assert!(
                matches!(parsed, Err(ParseError::InvalidNbformatMinor { .. })),
                "expected (c) for {rel}, got {parsed:?}"
            ),
            "d" => assert!(
                matches!(parsed, Err(ParseError::Mismatch { .. })),
                "expected (d) for {rel}, got {parsed:?}"
            ),
            other => panic!("unknown verdict {other} for {rel}"),
        }
    }
}
