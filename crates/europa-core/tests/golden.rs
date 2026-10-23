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

/// Builds the JSON escape sequence (backslash, `u`, four hex digits) for
/// `hex` at runtime so the test source never contains a literal escape.
fn esc(hex: &str) -> String {
    format!("\\{}{}", 'u', hex)
}

fn named(kind: &str, name: &str) -> Vec<u8> {
    notebooks(kind)
        .into_iter()
        .find(|(found, _)| found == name)
        .unwrap_or_else(|| panic!("golden/{kind}/{name} exists"))
        .1
}

fn saved(kind: &str, name: &str) -> String {
    serialize(&parse(&named(kind, name)).expect("fixture parses"))
}

fn lines_with(text: &str, wanted: &str) -> usize {
    text.lines()
        .filter(|line| line.trim().trim_end_matches(',') == wanted)
        .count()
}

#[test]
fn saving_again_changes_nothing() {
    for kind in ["foreign", "jupyter"] {
        for (name, bytes) in notebooks(kind) {
            let first = parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"));
            let text = serialize(&first);
            let second = parse(text.as_bytes())
                .unwrap_or_else(|error| panic!("{kind}/{name} reparses: {error:?}"));
            assert_eq!(first, second, "{kind}/{name} model equality");
            assert_eq!(text, serialize(&second), "{kind}/{name} output stability");
            if kind == "foreign" {
                insta::assert_snapshot!(format!("golden-foreign-{name}"), text);
            }
        }
    }
}

#[test]
fn crlf_input_is_written_with_lf_and_canonical_escapes() {
    let source = String::from_utf8(named("foreign", "layout.ipynb")).expect("UTF-8");
    assert!(source.contains("\r\n"));
    assert!(
        !source.replace("\r\n", "").contains('\n'),
        "every line break in the source is CRLF"
    );
    assert!(source.find("\"nbformat\"") < source.find("\"cells\""));
    assert!(source.contains("\\/"));
    assert!(source.contains(&esc("00e9")));

    let text = saved("foreign", "layout.ipynb");
    assert!(text.contains("caf\u{e9}"));
    assert!(!text.contains(&esc("00e9")));
    assert!(!text.contains('\r'));
    assert!(text.contains("a/b"));
    let position = |key: &str| text.find(&format!("\n \"{key}\"")).expect(key);
    assert!(position("cells") < position("metadata"));
    assert!(position("metadata") < position("nbformat"));
    assert!(position("nbformat") < position("nbformat_minor"));
}

#[test]
fn numbers_keep_their_spelling_in_every_position() {
    let keys = [
        ("exp_lower", "1e-05", "1e-05"),
        ("exp_upper", "1E5", "100000.0"),
        ("exp_plus", "1e+16", "1e+16"),
        ("trailing_zero", "1.50", "1.5"),
        ("tenth", "0.1", "0.1"),
        ("neg_zero_int", "-0", "0"),
        ("neg_zero_float", "-0.0", "-0.0"),
        ("underflow", "1e-400", "0.0"),
        ("beyond_u64", "18446744073709551616", "18446744073709551616"),
        (
            "precise_beyond",
            "1.00000000000000011102230246251565404236316680908203125",
            "1.0",
        ),
    ];
    let foreign = saved("foreign", "numbers.ipynb");
    let jupyter = saved("jupyter", "numbers.ipynb");
    for (key, input, rewritten) in keys {
        assert_eq!(
            lines_with(&foreign, &format!("\"{key}\": {input}")),
            9,
            "{key} in foreign"
        );
        assert_eq!(
            lines_with(&jupyter, &format!("\"{key}\": {rewritten}")),
            9,
            "{key} in jupyter"
        );
    }
}

#[test]
fn single_string_forms_stay_single_strings() {
    let value: serde_json::Value =
        serde_json::from_str(&saved("foreign", "text_forms.ipynb")).expect("output parses");
    for pointer in [
        "/cells/0/source",
        "/cells/1/source",
        "/cells/1/outputs/0/text",
        "/cells/1/outputs/1/data/text~1plain",
    ] {
        assert!(
            value
                .pointer(pointer)
                .is_some_and(serde_json::Value::is_string),
            "{pointer} stays a single string"
        );
    }
    assert!(value
        .pointer("/cells/1/outputs/1/data/text~1html")
        .is_some_and(serde_json::Value::is_array));
}
