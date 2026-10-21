//! Notebook model types.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::normalize::MultilineText;

/// Notebook document in nbformat v4.
///
/// Holds the format version, notebook-level metadata, and cells in order.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Notebook {
    /// Major format version, always 4 for accepted files.
    pub nbformat: u32,
    /// Minor format version.
    pub nbformat_minor: u32,
    /// Notebook-level metadata object.
    pub metadata: Map<String, Value>,
    /// Cells in document order.
    pub cells: Vec<Cell>,
}

/// Code cell with execution outputs.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CodeCell {
    /// In-memory identifier, never written back.
    #[serde(skip, default)]
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: Map<String, Value>,
    /// Execution outputs in order.
    pub outputs: Vec<Output>,
    /// Execution counter, or null when never executed.
    pub execution_count: Option<i64>,
}

/// Markdown cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MarkdownCell {
    /// In-memory identifier, never written back.
    #[serde(skip, default)]
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: Map<String, Value>,
}

/// Raw cell.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RawCell {
    /// In-memory identifier, never written back.
    #[serde(skip, default)]
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: Map<String, Value>,
}

/// Cell with an unrecognized `cell_type`.
///
/// Keeps the whole object unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownCell {
    /// Entire original object.
    #[serde(flatten)]
    pub value: Map<String, Value>,
    /// In-memory identifier, never written back.
    #[serde(skip, default)]
    pub(crate) mem_id: String,
}

/// Notebook cell, dispatched by `cell_type`.
#[derive(Debug, Clone, PartialEq)]
pub enum Cell {
    /// Code cell.
    Code(CodeCell),
    /// Markdown cell.
    Markdown(MarkdownCell),
    /// Raw cell.
    Raw(RawCell),
    /// Unrecognized cell kind, preserved as-is.
    Unknown(UnknownCell),
}

impl Serialize for Cell {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        unimplemented!()
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        unimplemented!()
    }
}

/// Stream output with a name and text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamOutput {
    /// Stream name such as stdout or stderr.
    pub name: String,
    /// Stream text.
    pub text: MultilineText,
}

/// Display data output with a MIME bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayDataOutput {
    /// MIME bundle mapping type to value.
    pub data: Map<String, Value>,
    /// Per-MIME metadata object.
    pub metadata: Map<String, Value>,
}

/// Execute result output with a MIME bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ExecuteResultOutput {
    /// MIME bundle mapping type to value.
    pub data: Map<String, Value>,
    /// Per-MIME metadata object.
    pub metadata: Map<String, Value>,
    /// Execution counter, or null.
    pub execution_count: Option<i64>,
}

/// Error output with traceback.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ErrorOutput {
    /// Exception name.
    pub ename: String,
    /// Exception value.
    pub evalue: String,
    /// Traceback lines.
    pub traceback: Vec<String>,
}

/// Output with an unrecognized `output_type`.
///
/// Keeps the whole object unchanged.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct UnknownOutput {
    /// Entire original object.
    #[serde(flatten)]
    pub value: Map<String, Value>,
}

/// Code cell output, dispatched by `output_type`.
#[derive(Debug, Clone, PartialEq)]
pub enum Output {
    /// Stream output.
    Stream(StreamOutput),
    /// Display data output.
    DisplayData(DisplayDataOutput),
    /// Execute result output.
    ExecuteResult(ExecuteResultOutput),
    /// Error output.
    Error(ErrorOutput),
    /// Unrecognized output kind, preserved as-is.
    Unknown(UnknownOutput),
}

impl Serialize for Output {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        unimplemented!()
    }
}

impl<'de> Deserialize<'de> for Output {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        unimplemented!()
    }
}
