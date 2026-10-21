//! Shared data types for Europa core models.
//!
//! Parse any nbformat v4 notebook without loss, keep unknown fields,
//! and write back deterministically.
//!
//! ```
//! let bytes = br#"{"nbformat":4,"nbformat_minor":5,"metadata":{},"cells":[]}"#;
//! let notebook = europa_core::parse(bytes).expect("loads");
//! let text = europa_core::serialize(&notebook);
//! assert!(text.ends_with('\n'));
//! ```
#![deny(missing_docs)]

pub mod error;
pub mod io;
pub mod normalize;
pub mod notebook;
pub mod pos;

pub use error::ParseError;
pub use io::{parse, serialize};
pub use normalize::MultilineText;
pub use notebook::{Cell, Notebook, Output};
pub use pos::{CellPos, ResolveError};
