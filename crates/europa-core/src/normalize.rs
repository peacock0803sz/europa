//! Multiline text handling and cell identifier assignment.

use serde::{Deserialize, Serialize};

/// Original representation of a multiline string value.
///
/// Jupyter allows `source` and stream `text` to be either a single
/// string or an array of strings. This enum keeps which form was used.
#[derive(Debug, Clone, PartialEq, Eq)]
enum MultilineRepr {
    /// Single string form.
    Single(String),
    /// Array-of-strings form, preserving element boundaries.
    Multiple(Vec<String>),
}

/// Multiline text with its original representation preserved.
///
/// Holds whether the input was a string or an array of strings and
/// writes back the same form. Reading uses the normalized text with
/// carriage returns resolved.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultilineText {
    raw: MultilineRepr,
    /// Normalized reading text, computed on deserialization.
    normalized: String,
}

impl MultilineText {
    /// Normalized reading text with CRLF resolved to LF.
    #[must_use]
    pub fn text(&self) -> &str {
        unimplemented!()
    }
}

impl Serialize for MultilineText {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match &self.raw {
            MultilineRepr::Single(s) => serializer.serialize_str(s),
            MultilineRepr::Multiple(parts) => parts.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for MultilineText {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Helper {
            Single(String),
            Multiple(Vec<String>),
        }

        let helper = Helper::deserialize(deserializer)?;
        let raw = match helper {
            Helper::Single(s) => MultilineRepr::Single(s),
            Helper::Multiple(parts) => MultilineRepr::Multiple(parts),
        };
        Ok(Self {
            raw,
            normalized: String::new(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn roundtrip(value: serde_json::Value) -> MultilineText {
        let parsed: MultilineText =
            serde_json::from_value(value.clone()).expect("accepts string or array");
        let back = serde_json::to_value(&parsed).expect("serializes back");
        assert_eq!(back, value, "original representation survives");
        parsed
    }

    #[test]
    fn accepts_string_and_array_forms() {
        let single = roundtrip(serde_json::json!("hello\nworld"));
        assert_eq!(
            single,
            MultilineText {
                raw: MultilineRepr::Single("hello\nworld".to_owned()),
                normalized: String::new(),
            }
        );

        let multi = roundtrip(serde_json::json!(["hello\n", "world"]));
        assert_eq!(
            multi,
            MultilineText {
                raw: MultilineRepr::Multiple(vec!["hello\n".to_owned(), "world".to_owned()]),
                normalized: String::new(),
            }
        );
    }

    #[test]
    fn preserves_crlf_and_empty_forms() {
        roundtrip(serde_json::json!("a\r\nb\r\n"));
        roundtrip(serde_json::json!(["a\r\n", "b\r\n"]));
        roundtrip(serde_json::json!(["a\r", "\nb"]));
        roundtrip(serde_json::json!(""));
        roundtrip(serde_json::json!([]));
    }
}
