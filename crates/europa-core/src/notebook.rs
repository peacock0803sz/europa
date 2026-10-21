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
    /// Unknown top-level fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown cell fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown cell fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown cell fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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

impl Cell {
    /// In-memory cell identifier for position references.
    ///
    /// Assigned during parsing and never written back to files.
    ///
    /// ```
    /// let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"markdown","metadata":{},"source":"hi"}]}"#;
    /// let notebook = europa_core::parse(bytes).expect("loads");
    /// assert_eq!(notebook.cells[0].id(), "cell-1");
    /// ```
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Code(inner) => &inner.mem_id,
            Self::Markdown(inner) => &inner.mem_id,
            Self::Raw(inner) => &inner.mem_id,
            Self::Unknown(inner) => &inner.mem_id,
        }
    }
}

impl Serialize for Cell {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Code(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert("cell_type".to_owned(), Value::String("code".to_owned()));
                }
                value.serialize(serializer)
            }
            Self::Markdown(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert("cell_type".to_owned(), Value::String("markdown".to_owned()));
                }
                value.serialize(serializer)
            }
            Self::Raw(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert("cell_type".to_owned(), Value::String("raw".to_owned()));
                }
                value.serialize(serializer)
            }
            Self::Unknown(inner) => inner.value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Cell {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut map = Map::deserialize(deserializer)?;
        let kind = match map.get("cell_type") {
            Some(Value::String(s)) => s.clone(),
            _ => {
                return Err(serde::de::Error::custom("missing or invalid cell_type"));
            }
        };
        match kind.as_str() {
            "code" | "markdown" | "raw" => {
                map.remove("cell_type");
                let value = Value::Object(map);
                match kind.as_str() {
                    "code" => {
                        let inner: CodeCell =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::Code(inner))
                    }
                    "markdown" => {
                        let inner: MarkdownCell =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::Markdown(inner))
                    }
                    _ => {
                        let inner: RawCell =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::Raw(inner))
                    }
                }
            }
            _ => Ok(Self::Unknown(UnknownCell {
                value: map,
                mem_id: String::new(),
            })),
        }
    }
}

/// Stream output with a name and text.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StreamOutput {
    /// Stream name such as stdout or stderr.
    pub name: String,
    /// Stream text.
    pub text: MultilineText,
    /// Unknown output fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
}

/// Display data output with a MIME bundle.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DisplayDataOutput {
    /// MIME bundle mapping type to value.
    pub data: Map<String, Value>,
    /// Per-MIME metadata object.
    pub metadata: Map<String, Value>,
    /// Unknown output fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown output fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    /// Unknown output fields, preserved unchanged.
    #[serde(flatten)]
    pub extra: Map<String, Value>,
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
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::Stream(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert("output_type".to_owned(), Value::String("stream".to_owned()));
                }
                value.serialize(serializer)
            }
            Self::DisplayData(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert(
                        "output_type".to_owned(),
                        Value::String("display_data".to_owned()),
                    );
                }
                value.serialize(serializer)
            }
            Self::ExecuteResult(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert(
                        "output_type".to_owned(),
                        Value::String("execute_result".to_owned()),
                    );
                }
                value.serialize(serializer)
            }
            Self::Error(inner) => {
                let mut value = serde_json::to_value(inner).map_err(serde::ser::Error::custom)?;
                if let Value::Object(ref mut map) = value {
                    map.insert("output_type".to_owned(), Value::String("error".to_owned()));
                }
                value.serialize(serializer)
            }
            Self::Unknown(inner) => inner.value.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Output {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let mut map = Map::deserialize(deserializer)?;
        let kind = match map.get("output_type") {
            Some(Value::String(s)) => s.clone(),
            _ => {
                return Err(serde::de::Error::custom("missing or invalid output_type"));
            }
        };
        match kind.as_str() {
            "stream" | "display_data" | "execute_result" | "error" => {
                map.remove("output_type");
                let value = Value::Object(map);
                match kind.as_str() {
                    "stream" => {
                        let inner: StreamOutput =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::Stream(inner))
                    }
                    "display_data" => {
                        let inner: DisplayDataOutput =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::DisplayData(inner))
                    }
                    "execute_result" => {
                        let inner: ExecuteResultOutput =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::ExecuteResult(inner))
                    }
                    _ => {
                        let inner: ErrorOutput =
                            serde_json::from_value(value).map_err(serde::de::Error::custom)?;
                        Ok(Self::Error(inner))
                    }
                }
            }
            _ => Ok(Self::Unknown(UnknownOutput { value: map })),
        }
    }
}
