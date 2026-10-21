//! Identifier assignment checks.

use std::path::PathBuf;

use europa_core::parse;

fn load(name: &str) -> europa_core::Notebook {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("text_and_ids")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture {name}"));
    parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"))
}

fn ids(notebook: &europa_core::Notebook) -> Vec<String> {
    notebook
        .cells
        .iter()
        .map(|cell| cell.id().to_owned())
        .collect()
}

#[test]
fn assigns_deterministic_ids() {
    let notebook = load("ids_mixed.ipynb");
    assert_eq!(
        ids(&notebook),
        vec![
            "a".to_owned(),
            "cell-2".to_owned(),
            "cell-3".to_owned(),
            "cell-1".to_owned(),
            "cell-4".to_owned(),
            "cell-5".to_owned(),
            "cell-6".to_owned(),
        ]
    );
    insta::assert_snapshot!("cell-ids-mixed", ids(&notebook).join(","));

    let notebook = load("unknown_ids.ipynb");
    assert_eq!(
        ids(&notebook),
        vec![
            "keep-me".to_owned(),
            "cell-1".to_owned(),
            "cell-2".to_owned(),
        ]
    );

    let notebook = load("no_ids.ipynb");
    assert_eq!(
        ids(&notebook),
        vec![
            "cell-1".to_owned(),
            "cell-2".to_owned(),
            "cell-3".to_owned(),
        ]
    );
    let mut sorted = ids(&notebook);
    sorted.sort();
    sorted.dedup();
    assert_eq!(sorted.len(), 3, "identifiers are unique");
}
