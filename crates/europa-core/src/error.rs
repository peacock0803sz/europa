//! Parse errors for notebook loading.

use std::fmt;

/// Reason for rejecting notebook input.
///
/// Each variant corresponds to one condition in the specification:
/// invalid JSON, invalid `nbformat`, invalid `nbformat_minor`,
/// or a structural mismatch of required fields.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ParseError {
    /// Input is not valid JSON.
    ///
    /// Covers malformed JSON, byte order marks, invalid UTF-8,
    /// unpaired surrogates, duplicate keys, and excessive nesting.
    /// Location is the line and column in the input, both starting at 1.
    InvalidJson {
        /// One-based line number in the input.
        line: usize,
        /// One-based column number in the input.
        column: usize,
        /// Human readable explanation.
        message: String,
    },
    /// `nbformat` is missing, not an integer, or not 4.
    ///
    /// Location is a JSON Pointer path such as `/nbformat`.
    InvalidNbformat {
        /// JSON Pointer path to the offending value.
        path: String,
        /// Human readable explanation.
        message: String,
    },
    /// `nbformat_minor` is missing or not a non-negative integer fitting in `u32`.
    ///
    /// Location is a JSON Pointer path such as `/nbformat_minor`.
    InvalidNbformatMinor {
        /// JSON Pointer path to the offending value.
        path: String,
        /// Human readable explanation.
        message: String,
    },
    /// Required structure does not match the expected notebook shape.
    ///
    /// Covers a non-object root and missing or mistyped required fields.
    /// Location is a JSON Pointer path to the offending value.
    Mismatch {
        /// JSON Pointer path to the offending value.
        path: String,
        /// Human readable explanation.
        message: String,
    },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidJson {
                line,
                column,
                message,
            } => write!(f, "invalid JSON at {line}:{column}: {message}"),
            Self::InvalidNbformat { path, message } => {
                write!(f, "invalid nbformat at {path}: {message}")
            }
            Self::InvalidNbformatMinor { path, message } => {
                write!(f, "invalid nbformat_minor at {path}: {message}")
            }
            Self::Mismatch { path, message } => {
                write!(f, "structural mismatch at {path}: {message}")
            }
        }
    }
}

impl std::error::Error for ParseError {}
