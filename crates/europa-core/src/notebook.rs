//! Notebook model types.

use serde::{Deserialize, Serialize};

use crate::canonical::{is_split_mime, split_lines};
use crate::error::ParseError;
use crate::json::{JsonMap, JsonValue};
use crate::normalize::MultilineText;

/// Notebook document in nbformat v4.
///
/// Holds the format version, notebook-level metadata, and cells in order.
///
/// Use [`crate::parse`] to load notebooks. It is the only supported entry
/// point: it validates the input and assigns cell identifiers. The serde
/// `Deserialize` impl skips both, leaving every [`Cell::id`] empty. The serde
/// impls also read and write numbers as values, so only [`crate::parse`] and
/// [`crate::serialize`] keep number literals as written.
#[derive(Debug, Clone, PartialEq)]
pub struct Notebook {
    /// Major format version, always 4 for accepted files.
    pub nbformat: u32,
    /// Minor format version.
    pub nbformat_minor: u32,
    /// Notebook-level metadata object.
    pub metadata: JsonMap,
    /// Cells in document order.
    pub cells: Vec<Cell>,
    /// Unknown top-level fields, preserved unchanged.
    pub extra: JsonMap,
}

/// Code cell with execution outputs.
#[derive(Debug, Clone, PartialEq)]
pub struct CodeCell {
    /// In-memory identifier, never written back.
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: JsonMap,
    /// Execution outputs in order.
    pub outputs: Vec<Output>,
    /// Execution counter, or null when never executed.
    pub execution_count: Option<i64>,
    /// Unknown cell fields, preserved unchanged.
    pub extra: JsonMap,
}

/// Markdown cell.
#[derive(Debug, Clone, PartialEq)]
pub struct MarkdownCell {
    /// In-memory identifier, never written back.
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: JsonMap,
    /// Unknown cell fields, preserved unchanged.
    pub extra: JsonMap,
}

/// Raw cell.
#[derive(Debug, Clone, PartialEq)]
pub struct RawCell {
    /// In-memory identifier, never written back.
    pub(crate) mem_id: String,
    /// Cell source text.
    pub source: MultilineText,
    /// Cell-level metadata object.
    pub metadata: JsonMap,
    /// Unknown cell fields, preserved unchanged.
    pub extra: JsonMap,
}

/// Cell with an unrecognized `cell_type`.
///
/// Keeps the whole object unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownCell {
    /// Entire original object.
    pub value: JsonMap,
    /// In-memory identifier, never written back.
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
    /// Assigned by [`crate::parse`] and never written back to files.
    /// Empty for notebooks deserialized directly with serde.
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

/// Stream output with a name and text.
#[derive(Debug, Clone, PartialEq)]
pub struct StreamOutput {
    /// Stream name such as stdout or stderr.
    pub name: String,
    /// Stream text.
    pub text: MultilineText,
    /// Unknown output fields, preserved unchanged.
    pub extra: JsonMap,
    /// Whether the output counts as new; see [`Output::is_new`].
    pub(crate) origin: OutputOrigin,
}

/// Display data output with a MIME bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct DisplayDataOutput {
    /// MIME bundle mapping type to value.
    pub data: JsonMap,
    /// Per-MIME metadata object.
    pub metadata: JsonMap,
    /// Unknown output fields, preserved unchanged.
    pub extra: JsonMap,
    /// Whether the output counts as new; see [`Output::is_new`].
    pub(crate) origin: OutputOrigin,
    /// The display id the output was shown with, if any. Never saved.
    pub(crate) binding: Option<DisplayBinding>,
}

/// A display id together with the generation of the execution state it was
/// shown in.
///
/// The id is a transient hint of the kernel, so it is neither public nor saved.
/// It sits on the output rather than in the execution state because it has to
/// vanish with the output however that is removed, even by a caller that
/// empties `outputs` directly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DisplayBinding {
    pub(crate) id: String,
    pub(crate) generation: u64,
}

/// Execute result output with a MIME bundle.
#[derive(Debug, Clone, PartialEq)]
pub struct ExecuteResultOutput {
    /// MIME bundle mapping type to value.
    pub data: JsonMap,
    /// Per-MIME metadata object.
    pub metadata: JsonMap,
    /// Execution counter, or null.
    pub execution_count: Option<i64>,
    /// Unknown output fields, preserved unchanged.
    pub extra: JsonMap,
    /// Whether the output counts as new; see [`Output::is_new`].
    pub(crate) origin: OutputOrigin,
}

/// Error output with traceback.
#[derive(Debug, Clone, PartialEq)]
pub struct ErrorOutput {
    /// Exception name.
    pub ename: String,
    /// Exception value.
    pub evalue: String,
    /// Traceback lines.
    pub traceback: Vec<String>,
    /// Unknown output fields, preserved unchanged.
    pub extra: JsonMap,
    /// Whether the output counts as new; see [`Output::is_new`].
    pub(crate) origin: OutputOrigin,
}

/// Output with an unrecognized `output_type`.
///
/// Keeps the whole object unchanged.
#[derive(Debug, Clone, PartialEq)]
pub struct UnknownOutput {
    /// Entire original object.
    pub value: JsonMap,
}

impl StreamOutput {
    /// Rebuild as a new output whose text is the reading text after `f`.
    ///
    /// The name and unknown fields stay. Saving then splits the text into
    /// lines, which a loaded string or array form cannot keep once changed.
    pub(crate) fn rebuild_text(&mut self, f: impl FnOnce(&mut String)) {
        let mut body = self.text.text().to_owned();
        f(&mut body);
        self.text = MultilineText::single(body);
        self.origin = OutputOrigin::New;
    }
}

/// Code cell output, dispatched by `output_type`.
///
/// Build outputs for a code cell with the `Output::new_*` constructors, so
/// that saving splits their text into lines the way nbformat does; see
/// [`Output::is_new`]. A loaded stream output that [`crate::apply_iopub`]
/// appended to is split the same way.
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

/// Where an output came from.
///
/// [`crate::parse`] and serde `Deserialize` make every output `Loaded` and
/// `Output::new_*` makes `New` ones. A loaded stream output becomes `New` once
/// [`crate::apply_iopub`] appends to it. [`crate::serialize`] leaves the model
/// unchanged, so a saved notebook keeps its `New` outputs until it is parsed
/// again, which turns all of them into `Loaded`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum OutputOrigin {
    /// Read by [`crate::parse`] or serde. Saving splits nothing and writes its
    /// strings and arrays of strings back as they were; number literals
    /// survive only [`crate::parse`], see [`JsonValue`].
    Loaded,
    /// Built by an `Output::new_*` constructor or rebuilt by appending to a
    /// loaded stream; saving splits its text into lines and writes every
    /// other value, numbers included, as given.
    New,
}

impl Output {
    /// Whether this output was built with an `Output::new_*` constructor.
    ///
    /// A loaded stream output that [`crate::apply_iopub`] appended to also
    /// reports true. Only such outputs have their text split into lines on
    /// save; see [`crate::serialize`]. Other outputs from [`crate::parse`] or
    /// from serde `Deserialize`, such as a kernel message read with
    /// `serde_json`, report false. Saving splits nothing in them and writes
    /// their strings and arrays of strings back as they were. Number literals
    /// survive only [`crate::parse`], because serde rebuilds numbers from
    /// `f64`; see [`JsonValue`]. Always false for [`Output::Unknown`].
    ///
    /// Saving does not change the flag, and parsing the saved text gives
    /// outputs that report false. The flag takes part in `==`, so a new output
    /// never equals one that was read, even with the same content.
    ///
    /// ```
    /// use europa_core::Output;
    ///
    /// assert!(Output::new_stream("stdout", "hi\n").is_new());
    /// ```
    #[must_use]
    pub fn is_new(&self) -> bool {
        let origin = match self {
            Self::Stream(inner) => inner.origin,
            Self::DisplayData(inner) => inner.origin,
            Self::ExecuteResult(inner) => inner.origin,
            Self::Error(inner) => inner.origin,
            Self::Unknown(_) => return false,
        };
        origin == OutputOrigin::New
    }

    /// Build a new stream output; see [`Output::is_new`].
    ///
    /// The text is kept as one string; saving splits it into lines the way
    /// nbformat does. Add the result to a code cell's `outputs`.
    ///
    /// ```
    /// use europa_core::{Cell, Output};
    ///
    /// let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[{"cell_type":"code","id":"a","metadata":{},"source":"","outputs":[],"execution_count":null}]}"#;
    /// let mut notebook = europa_core::parse(bytes).expect("loads");
    /// if let Cell::Code(code) = &mut notebook.cells[0] {
    ///     code.outputs.push(Output::new_stream("stdout", "a\nb\n"));
    /// }
    /// assert!(europa_core::serialize(&notebook).contains("\"a\\n\",\n"));
    /// ```
    #[must_use]
    pub fn new_stream(name: impl Into<String>, text: impl Into<String>) -> Output {
        Output::Stream(StreamOutput {
            name: name.into(),
            text: MultilineText::single(text.into()),
            extra: JsonMap::new(),
            origin: OutputOrigin::New,
        })
    }

    /// Build a new display data output; see [`Output::is_new`].
    ///
    /// Saving splits the string values in `data` whose MIME type starts with
    /// `text/`, or is `application/javascript` or `image/svg+xml`, into lines
    /// the way nbformat does. Every other value, such as `application/json`
    /// data, a base64 image or anything in `metadata`, is written as given.
    /// Numbers keep the literal they were built with, so pass the spelling of
    /// Python's `repr` to match what nbformat writes.
    ///
    /// ```
    /// use europa_core::{JsonMap, Output};
    ///
    /// let mut data = JsonMap::new();
    /// data.insert("text/plain".to_owned(), "one\ntwo".into());
    /// let output = Output::new_display_data(data, JsonMap::new());
    /// assert!(output.is_new());
    /// ```
    #[must_use]
    pub fn new_display_data(data: JsonMap, metadata: JsonMap) -> Output {
        Output::DisplayData(DisplayDataOutput {
            data,
            metadata,
            extra: JsonMap::new(),
            origin: OutputOrigin::New,
            binding: None,
        })
    }

    /// Build a new execute result output; see [`Output::is_new`].
    ///
    /// Saving writes `data` and `metadata` as [`Output::new_display_data`]
    /// describes.
    #[must_use]
    pub fn new_execute_result(
        data: JsonMap,
        metadata: JsonMap,
        execution_count: Option<i64>,
    ) -> Output {
        Output::ExecuteResult(ExecuteResultOutput {
            data,
            metadata,
            execution_count,
            extra: JsonMap::new(),
            origin: OutputOrigin::New,
        })
    }

    /// Build a new error output; see [`Output::is_new`].
    ///
    /// Saving writes every field as given, as nbformat does for errors.
    #[must_use]
    pub fn new_error(
        ename: impl Into<String>,
        evalue: impl Into<String>,
        traceback: Vec<String>,
    ) -> Output {
        Output::Error(ErrorOutput {
            ename: ename.into(),
            evalue: evalue.into(),
            traceback,
            extra: JsonMap::new(),
            origin: OutputOrigin::New,
        })
    }
}

/// Text of a stream output as it is written.
///
/// Only a new output holding one string is split into lines. Everything read
/// from a file keeps the form it had.
fn stream_text_to_json(text: &MultilineText, origin: OutputOrigin) -> JsonValue {
    match (origin, text.as_single()) {
        (OutputOrigin::New, Some(single)) => lines_to_json(single),
        _ => text.to_json(),
    }
}

/// MIME bundle as it is written.
///
/// Only a new output has its text-like string values split into lines.
fn mime_bundle_to_json(data: &JsonMap, origin: OutputOrigin) -> JsonValue {
    let mut bundle = data.clone();
    if origin == OutputOrigin::New {
        for (mime, value) in &mut bundle {
            if let (true, JsonValue::String(text)) = (is_split_mime(mime), &*value) {
                *value = lines_to_json(text);
            }
        }
    }
    JsonValue::Object(bundle)
}

fn lines_to_json(text: &str) -> JsonValue {
    JsonValue::Array(split_lines(text).into_iter().map(JsonValue::from).collect())
}

// Conversion between the model and the literal-preserving JSON tree.

fn mismatch(path: &str, message: &str) -> ParseError {
    ParseError::Mismatch {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

fn child(path: &str, key: &str) -> String {
    format!("{path}/{key}")
}

fn take_object(
    map: &mut JsonMap,
    key: &str,
    path: &str,
    message: &str,
) -> Result<JsonMap, ParseError> {
    match map.remove(key) {
        Some(JsonValue::Object(inner)) => Ok(inner),
        _ => Err(mismatch(&child(path, key), message)),
    }
}

fn take_string(
    map: &mut JsonMap,
    key: &str,
    path: &str,
    message: &str,
) -> Result<String, ParseError> {
    match map.remove(key) {
        Some(JsonValue::String(text)) => Ok(text),
        _ => Err(mismatch(&child(path, key), message)),
    }
}

fn take_text(map: &mut JsonMap, key: &str, path: &str) -> Result<MultilineText, ParseError> {
    map.remove(key)
        .as_ref()
        .and_then(MultilineText::from_json)
        .ok_or_else(|| mismatch(&child(path, key), "must be a string or an array of strings"))
}

fn take_count(map: &mut JsonMap, path: &str) -> Result<Option<i64>, ParseError> {
    match map.remove("execution_count") {
        Some(JsonValue::Null) => Ok(None),
        Some(JsonValue::Number(number)) if number.as_i64().is_some() => Ok(number.as_i64()),
        _ => Err(mismatch(
            &child(path, "execution_count"),
            "execution_count must be an i64 integer or null",
        )),
    }
}

fn count_to_json(count: Option<i64>) -> JsonValue {
    count.map_or(JsonValue::Null, JsonValue::from)
}

/// Conversion of a struct to and from an object without its kind key.
///
/// `UnknownCell` and `UnknownOutput` convert the whole object instead.
trait JsonMapConvert: Sized {
    fn from_json_map(map: JsonMap, path: &str) -> Result<Self, ParseError>;
    fn to_json_map(&self) -> JsonMap;
}

macro_rules! serde_via_map {
    ($($ty:ty),* $(,)?) => {$(
        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.to_json_map().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let map = JsonMap::deserialize(deserializer)?;
                Self::from_json_map(map, "").map_err(serde::de::Error::custom)
            }
        }
    )*};
}

serde_via_map!(
    CodeCell,
    MarkdownCell,
    RawCell,
    UnknownCell,
    StreamOutput,
    DisplayDataOutput,
    ExecuteResultOutput,
    ErrorOutput,
    UnknownOutput,
);

macro_rules! serde_via_json {
    ($($ty:ty),* $(,)?) => {$(
        impl Serialize for $ty {
            fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
            where
                S: serde::Serializer,
            {
                self.to_json().serialize(serializer)
            }
        }

        impl<'de> Deserialize<'de> for $ty {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: serde::Deserializer<'de>,
            {
                let value = JsonValue::deserialize(deserializer)?;
                Self::from_json(value, "").map_err(serde::de::Error::custom)
            }
        }
    )*};
}

serde_via_json!(Cell, Output);

impl Serialize for Notebook {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        self.to_json().serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for Notebook {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let value = JsonValue::deserialize(deserializer)?;
        Self::from_json(value).map_err(serde::de::Error::custom)
    }
}

impl Notebook {
    pub(crate) fn from_json(value: JsonValue) -> Result<Notebook, ParseError> {
        let JsonValue::Object(mut map) = value else {
            return Err(mismatch("", "root must be an object"));
        };
        let version = |map: &mut JsonMap, key: &str| match map.remove(key) {
            Some(JsonValue::Number(number)) => number.as_u64().and_then(|n| u32::try_from(n).ok()),
            _ => None,
        };
        let nbformat =
            version(&mut map, "nbformat").ok_or_else(|| ParseError::InvalidNbformat {
                path: "/nbformat".to_owned(),
                message: "nbformat must be integer 4".to_owned(),
            })?;
        let nbformat_minor = version(&mut map, "nbformat_minor").ok_or_else(|| {
            ParseError::InvalidNbformatMinor {
                path: "/nbformat_minor".to_owned(),
                message: "nbformat_minor must be a u32 integer".to_owned(),
            }
        })?;
        let metadata = take_object(&mut map, "metadata", "", "metadata must be an object")?;
        let items = match map.remove("cells") {
            Some(JsonValue::Array(items)) => items,
            _ => return Err(mismatch("/cells", "cells must be an array")),
        };
        let cells = items
            .into_iter()
            .enumerate()
            .map(|(idx, item)| Cell::from_json(item, &format!("/cells/{idx}")))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Notebook {
            nbformat,
            nbformat_minor,
            metadata,
            cells,
            extra: map,
        })
    }

    pub(crate) fn to_json(&self) -> JsonValue {
        let mut map = self.extra.clone();
        map.insert("nbformat".to_owned(), u64::from(self.nbformat).into());
        map.insert(
            "nbformat_minor".to_owned(),
            u64::from(self.nbformat_minor).into(),
        );
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map.insert(
            "cells".to_owned(),
            self.cells
                .iter()
                .map(Cell::to_json)
                .collect::<Vec<_>>()
                .into(),
        );
        JsonValue::Object(map)
    }
}

/// Read the kind of an object and return it with the whole object.
///
/// The kind key stays in the object. Callers remove it for the kinds they
/// know and keep it for unknown kinds, which preserve the object as it was.
fn take_kind(
    value: JsonValue,
    kind_key: &str,
    path: &str,
    noun: &str,
) -> Result<(String, JsonMap), ParseError> {
    let JsonValue::Object(map) = value else {
        return Err(mismatch(path, &format!("{noun} must be an object")));
    };
    match map.get(kind_key) {
        Some(JsonValue::String(kind)) => Ok((kind.clone(), map)),
        _ => Err(mismatch(
            &child(path, kind_key),
            &format!("{kind_key} must be a string"),
        )),
    }
}

impl Cell {
    pub(crate) fn from_json(value: JsonValue, path: &str) -> Result<Cell, ParseError> {
        let (kind, mut map) = take_kind(value, "cell_type", path, "cell")?;
        match kind.as_str() {
            "code" => {
                map.remove("cell_type");
                CodeCell::from_json_map(map, path).map(Cell::Code)
            }
            "markdown" => {
                map.remove("cell_type");
                MarkdownCell::from_json_map(map, path).map(Cell::Markdown)
            }
            "raw" => {
                map.remove("cell_type");
                RawCell::from_json_map(map, path).map(Cell::Raw)
            }
            _ => UnknownCell::from_json_map(map, path).map(Cell::Unknown),
        }
    }

    pub(crate) fn to_json(&self) -> JsonValue {
        let (kind, mut map) = match self {
            Self::Code(inner) => ("code", inner.to_json_map()),
            Self::Markdown(inner) => ("markdown", inner.to_json_map()),
            Self::Raw(inner) => ("raw", inner.to_json_map()),
            Self::Unknown(inner) => return JsonValue::Object(inner.to_json_map()),
        };
        map.insert("cell_type".to_owned(), kind.into());
        JsonValue::Object(map)
    }
}

impl Output {
    pub(crate) fn from_json(value: JsonValue, path: &str) -> Result<Output, ParseError> {
        let (kind, mut map) = take_kind(value, "output_type", path, "output")?;
        match kind.as_str() {
            "stream" => {
                map.remove("output_type");
                StreamOutput::from_json_map(map, path).map(Output::Stream)
            }
            "display_data" => {
                map.remove("output_type");
                DisplayDataOutput::from_json_map(map, path).map(Output::DisplayData)
            }
            "execute_result" => {
                map.remove("output_type");
                ExecuteResultOutput::from_json_map(map, path).map(Output::ExecuteResult)
            }
            "error" => {
                map.remove("output_type");
                ErrorOutput::from_json_map(map, path).map(Output::Error)
            }
            _ => UnknownOutput::from_json_map(map, path).map(Output::Unknown),
        }
    }

    pub(crate) fn to_json(&self) -> JsonValue {
        let (kind, mut map) = match self {
            Self::Stream(inner) => ("stream", inner.to_json_map()),
            Self::DisplayData(inner) => ("display_data", inner.to_json_map()),
            Self::ExecuteResult(inner) => ("execute_result", inner.to_json_map()),
            Self::Error(inner) => ("error", inner.to_json_map()),
            Self::Unknown(inner) => return JsonValue::Object(inner.to_json_map()),
        };
        map.insert("output_type".to_owned(), kind.into());
        JsonValue::Object(map)
    }
}

impl JsonMapConvert for CodeCell {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let source = take_text(&mut map, "source", path)?;
        let metadata = take_object(
            &mut map,
            "metadata",
            path,
            "cell metadata must be an object",
        )?;
        let outputs_path = child(path, "outputs");
        let outputs = match map.remove("outputs") {
            Some(JsonValue::Array(items)) => items
                .into_iter()
                .enumerate()
                .map(|(idx, item)| Output::from_json(item, &format!("{outputs_path}/{idx}")))
                .collect::<Result<Vec<_>, _>>()?,
            _ => return Err(mismatch(&outputs_path, "outputs must be an array")),
        };
        let execution_count = take_count(&mut map, path)?;
        Ok(CodeCell {
            mem_id: String::new(),
            source,
            metadata,
            outputs,
            execution_count,
            extra: map,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert("source".to_owned(), self.source.to_json());
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map.insert(
            "outputs".to_owned(),
            self.outputs
                .iter()
                .map(Output::to_json)
                .collect::<Vec<_>>()
                .into(),
        );
        map.insert(
            "execution_count".to_owned(),
            count_to_json(self.execution_count),
        );
        map
    }
}

impl JsonMapConvert for MarkdownCell {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let source = take_text(&mut map, "source", path)?;
        let metadata = take_object(
            &mut map,
            "metadata",
            path,
            "cell metadata must be an object",
        )?;
        Ok(MarkdownCell {
            mem_id: String::new(),
            source,
            metadata,
            extra: map,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert("source".to_owned(), self.source.to_json());
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map
    }
}

impl JsonMapConvert for RawCell {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let source = take_text(&mut map, "source", path)?;
        let metadata = take_object(
            &mut map,
            "metadata",
            path,
            "cell metadata must be an object",
        )?;
        Ok(RawCell {
            mem_id: String::new(),
            source,
            metadata,
            extra: map,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert("source".to_owned(), self.source.to_json());
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map
    }
}

impl JsonMapConvert for UnknownCell {
    fn from_json_map(map: JsonMap, _path: &str) -> Result<Self, ParseError> {
        Ok(UnknownCell {
            value: map,
            mem_id: String::new(),
        })
    }

    fn to_json_map(&self) -> JsonMap {
        self.value.clone()
    }
}

impl JsonMapConvert for StreamOutput {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let name = take_string(&mut map, "name", path, "stream name must be a string")?;
        let text = take_text(&mut map, "text", path)?;
        Ok(StreamOutput {
            name,
            text,
            extra: map,
            origin: OutputOrigin::Loaded,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert("name".to_owned(), self.name.as_str().into());
        map.insert(
            "text".to_owned(),
            stream_text_to_json(&self.text, self.origin),
        );
        map
    }
}

impl JsonMapConvert for DisplayDataOutput {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let data = take_object(&mut map, "data", path, "data must be an object")?;
        let metadata = take_object(
            &mut map,
            "metadata",
            path,
            "output metadata must be an object",
        )?;
        Ok(DisplayDataOutput {
            data,
            metadata,
            extra: map,
            origin: OutputOrigin::Loaded,
            binding: None,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert(
            "data".to_owned(),
            mime_bundle_to_json(&self.data, self.origin),
        );
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map
    }
}

impl JsonMapConvert for ExecuteResultOutput {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let data = take_object(&mut map, "data", path, "data must be an object")?;
        let metadata = take_object(
            &mut map,
            "metadata",
            path,
            "output metadata must be an object",
        )?;
        let execution_count = take_count(&mut map, path)?;
        Ok(ExecuteResultOutput {
            data,
            metadata,
            execution_count,
            extra: map,
            origin: OutputOrigin::Loaded,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert(
            "data".to_owned(),
            mime_bundle_to_json(&self.data, self.origin),
        );
        map.insert("metadata".to_owned(), self.metadata.clone().into());
        map.insert(
            "execution_count".to_owned(),
            count_to_json(self.execution_count),
        );
        map
    }
}

impl JsonMapConvert for ErrorOutput {
    fn from_json_map(mut map: JsonMap, path: &str) -> Result<Self, ParseError> {
        let ename = take_string(&mut map, "ename", path, "ename must be a string")?;
        let evalue = take_string(&mut map, "evalue", path, "evalue must be a string")?;
        let traceback = match map.remove("traceback") {
            Some(JsonValue::Array(lines)) => lines
                .into_iter()
                .map(|line| match line {
                    JsonValue::String(text) => Some(text),
                    _ => None,
                })
                .collect::<Option<Vec<_>>>(),
            _ => None,
        }
        .ok_or_else(|| {
            mismatch(
                &child(path, "traceback"),
                "traceback must be an array of strings",
            )
        })?;
        Ok(ErrorOutput {
            ename,
            evalue,
            traceback,
            extra: map,
            origin: OutputOrigin::Loaded,
        })
    }

    fn to_json_map(&self) -> JsonMap {
        let mut map = self.extra.clone();
        map.insert("ename".to_owned(), self.ename.as_str().into());
        map.insert("evalue".to_owned(), self.evalue.as_str().into());
        map.insert(
            "traceback".to_owned(),
            self.traceback
                .iter()
                .map(|line| JsonValue::from(line.as_str()))
                .collect::<Vec<_>>()
                .into(),
        );
        map
    }
}

impl JsonMapConvert for UnknownOutput {
    fn from_json_map(map: JsonMap, _path: &str) -> Result<Self, ParseError> {
        Ok(UnknownOutput { value: map })
    }

    fn to_json_map(&self) -> JsonMap {
        self.value.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::json::read_validated;

    fn load(text: &str) -> Result<Notebook, ParseError> {
        Notebook::from_json(read_validated(text).expect("reads"))
    }

    fn mismatch_path(text: &str) -> String {
        match load(text) {
            Err(ParseError::Mismatch { path, .. }) => path,
            other => panic!("expected a mismatch, got {other:?}"),
        }
    }

    const HEAD: &str = r#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":"#;

    #[test]
    fn reports_missing_cells() {
        assert_eq!(
            mismatch_path(r#"{"nbformat":4,"nbformat_minor":5,"metadata":{}}"#),
            "/cells"
        );
    }

    #[test]
    fn reports_missing_execution_count() {
        let text =
            format!(r#"{HEAD}[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[]}}]}}"#);
        assert_eq!(mismatch_path(&text), "/cells/0/execution_count");
    }

    #[test]
    fn reports_missing_output_type() {
        let text = format!(
            r#"{HEAD}[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[{{}}],"execution_count":null}}]}}"#
        );
        assert_eq!(mismatch_path(&text), "/cells/0/outputs/0/output_type");
    }

    #[test]
    fn reports_numeric_source() {
        let text = format!(r#"{HEAD}[{{"cell_type":"raw","source":3,"metadata":{{}}}}]}}"#);
        assert_eq!(mismatch_path(&text), "/cells/0/source");
    }

    #[test]
    fn known_fields_and_kind_win_over_extra() {
        let mut extra = JsonMap::new();
        extra.insert("source".to_owned(), JsonValue::from(1_i64));
        extra.insert("cell_type".to_owned(), JsonValue::from("other"));
        extra.insert("kept".to_owned(), JsonValue::from(true));
        let cell = Cell::Code(CodeCell {
            mem_id: "cell-1".to_owned(),
            source: MultilineText::from_json(&JsonValue::from("a")).expect("string"),
            metadata: JsonMap::new(),
            outputs: Vec::new(),
            execution_count: None,
            extra,
        });
        let object = cell.to_json();
        let map = object.as_object().expect("object");
        assert_eq!(map["source"], JsonValue::from("a"));
        assert_eq!(map["cell_type"], JsonValue::from("code"));
        assert_eq!(map["kept"], JsonValue::from(true));
        assert!(!map.contains_key("id"), "in-memory id is never written");
    }

    #[test]
    fn serde_keeps_kind_keys_on_the_enums_only() {
        let notebook = load(&format!(
            r#"{HEAD}[{{"cell_type":"code","source":"","metadata":{{}},"outputs":[],"execution_count":null}}]}}"#
        ))
        .expect("loads");
        let Cell::Code(inner) = &notebook.cells[0] else {
            panic!("code cell");
        };
        let bare = serde_json::to_value(inner).expect("serializes");
        assert!(bare.get("cell_type").is_none());
        let tagged = serde_json::to_value(&notebook.cells[0]).expect("serializes");
        assert_eq!(tagged["cell_type"], "code");
    }

    #[test]
    fn plain_deserialize_leaves_ids_empty() {
        let text = format!(r#"{HEAD}[{{"cell_type":"markdown","source":"","metadata":{{}}}}]}}"#);
        let notebook: Notebook = serde_json::from_str(&text).expect("deserializes");
        assert_eq!(notebook.cells[0].id(), "");
    }
}
