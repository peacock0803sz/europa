//! Cell position types and resolution.

use crate::notebook::{Cell, Notebook};

/// Position inside a cell on normalized text.
///
/// Both line and column start at zero. Column counts Unicode scalar values.
///
/// ```
/// let pos = europa_core::CellPos {
///     cell_id: "cell-1".to_owned(),
///     line: 0,
///     col: 0,
/// };
/// assert_eq!(pos.line, 0);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct CellPos {
    /// In-memory cell identifier.
    pub cell_id: String,
    /// Zero-based line number.
    pub line: u32,
    /// Zero-based column in Unicode scalar values.
    pub col: u32,
}

/// Failure to resolve a cell position.
///
/// Returned when the referenced cell identifier does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolveError {
    /// Missing cell identifier.
    pub cell_id: String,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "unknown cell id: {}", self.cell_id)
    }
}

impl std::error::Error for ResolveError {}

impl Notebook {
    /// Find the cell referenced by a position.
    ///
    /// Looks up the in-memory cell identifier and returns the cell.
    ///
    /// ```
    /// let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","metadata":{},"source":"hi"}]}"#;
    /// let notebook = europa_core::parse(bytes).expect("loads");
    /// let pos = europa_core::CellPos {
    ///     cell_id: "cell-1".to_owned(),
    ///     line: 0,
    ///     col: 0,
    /// };
    /// assert!(notebook.resolve(&pos).is_ok());
    /// ```
    pub fn resolve(&self, pos: &CellPos) -> Result<&Cell, ResolveError> {
        self.cells
            .iter()
            .find(|cell| cell.id() == pos.cell_id)
            .ok_or_else(|| ResolveError {
                cell_id: pos.cell_id.clone(),
            })
    }
}
