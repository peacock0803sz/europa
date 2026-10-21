//! Notebook parsing and serialization entry points.

use crate::error::ParseError;
use crate::notebook::Notebook;

/// Parse notebook bytes into a notebook model.
///
/// Validates format conditions and rejects unsupported input with a reason.
#[allow(clippy::missing_panics_doc)]
pub fn parse(_bytes: &[u8]) -> Result<Notebook, ParseError> {
    unimplemented!()
}
