//! Cell position resolution checks.

use std::path::PathBuf;

use europa_core::{parse, CellPos};

fn load(name: &str) -> europa_core::Notebook {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("text_and_ids")
        .join(name);
    let bytes = std::fs::read(&path).unwrap_or_else(|_| panic!("missing fixture {name}"));
    parse(&bytes).unwrap_or_else(|error| panic!("{name} parses: {error:?}"))
}

#[test]
fn resolves_existing_and_numbered_ids() {
    let notebook = load("ids_mixed.ipynb");
    for cell in &notebook.cells {
        let pos = CellPos {
            cell_id: cell.id().to_owned(),
            line: 0,
            col: 0,
        };
        let found = notebook.resolve(&pos).expect("resolves own id");
        assert_eq!(found.id(), cell.id());
    }

    let notebook = load("unknown_ids.ipynb");
    for cell in &notebook.cells {
        let pos = CellPos {
            cell_id: cell.id().to_owned(),
            line: 0,
            col: 0,
        };
        assert!(notebook.resolve(&pos).is_ok());
    }
}

#[test]
fn resolves_all_cells_without_file_ids() {
    let notebook = load("no_ids.ipynb");
    assert_eq!(notebook.cells.len(), 3);
    for cell in &notebook.cells {
        let pos = CellPos {
            cell_id: cell.id().to_owned(),
            line: 0,
            col: 0,
        };
        assert!(notebook.resolve(&pos).is_ok(), "resolves {}", cell.id());
    }
}

#[test]
fn rejects_missing_identifier() {
    let notebook = load("no_ids.ipynb");
    let pos = CellPos {
        cell_id: "no-such-cell".to_owned(),
        line: 0,
        col: 0,
    };
    let error = notebook.resolve(&pos).expect_err("missing id fails");
    assert_eq!(error.cell_id, "no-such-cell");
}
