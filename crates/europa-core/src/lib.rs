//! Shared data types for Europa core models.
//!
//! Parse nbformat v4 notebooks that pass validation, preserving every field
//! value including unknown fields, and write them back deterministically
//! with sorted keys.
//!
//! Numbers keep the spelling they were read with, so `1E5` and `1.50` come
//! back unchanged. A notebook that Jupyter wrote is a fixed point of
//! [`parse`] followed by [`serialize`]: the saved bytes equal the input.
//!
//! ```
//! let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#;
//! let notebook = europa_core::parse(bytes).expect("loads");
//! let text = europa_core::serialize(&notebook);
//! assert!(text.ends_with('\n'));
//! ```
#![deny(missing_docs)]

mod canonical;
pub mod error;
pub mod io;
pub mod json;
pub mod normalize;
pub mod notebook;
pub mod pos;

pub use error::ParseError;
pub use io::{parse, serialize};
pub use json::{JsonMap, JsonNumber, JsonValue};
pub use normalize::MultilineText;
pub use notebook::{Cell, Notebook, Output};
pub use pos::{CellPos, ResolveError};
