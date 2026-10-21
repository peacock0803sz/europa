//! Multiline text handling and cell identifier assignment.

use serde::{Deserialize, Serialize};

/// Original representation of a multiline string value.
///
/// Jupyter allows `source` and stream `text` to be either a single
/// string or an array of strings. This enum keeps which form was used.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
enum MultilineRepr {
    /// Single string form.
    Single(String),
    /// Array-of-strings form, preserving element boundaries.
    Multiple(Vec<String>),
}

/// Multiline text with its original representation preserved.
///
/// Holds whether the input was a string or an array of strings and
/// writes back the same form. The normalized reading value is added
/// separately.
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(dead_code)]
pub struct MultilineText {
    raw: MultilineRepr,
}

impl Serialize for MultilineText {
    fn serialize<S>(&self, _serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        unimplemented!()
    }
}

impl<'de> Deserialize<'de> for MultilineText {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        unimplemented!()
    }
}
