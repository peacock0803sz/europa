//! Multiline text handling and cell identifier assignment.

use serde::{Deserialize, Serialize};

use crate::notebook::Cell;

/// Assign in-memory identifiers to cells in document order, in place.
///
/// The first cell carrying each non-empty string `id` keeps it. Every other
/// cell gets the next free `cell-N`, skipping any string `id` used anywhere
/// in the file. Called by [`crate::parse`]; the identifiers are never
/// written back.
#[allow(clippy::missing_panics_doc)]
pub(crate) fn assign_cell_ids(cells: &mut [Cell]) {
    use crate::json::JsonValue as Value;
    use std::collections::HashSet;

    fn file_id(cell: &Cell) -> Option<&str> {
        match cell {
            Cell::Code(inner) => inner.extra.get("id").and_then(Value::as_str),
            Cell::Markdown(inner) => inner.extra.get("id").and_then(Value::as_str),
            Cell::Raw(inner) => inner.extra.get("id").and_then(Value::as_str),
            Cell::Unknown(inner) => inner.value.get("id").and_then(Value::as_str),
        }
    }

    fn mem_id_mut(cell: &mut Cell) -> &mut String {
        match cell {
            Cell::Code(inner) => &mut inner.mem_id,
            Cell::Markdown(inner) => &mut inner.mem_id,
            Cell::Raw(inner) => &mut inner.mem_id,
            Cell::Unknown(inner) => &mut inner.mem_id,
        }
    }

    let mut all_strings: HashSet<String> = HashSet::new();
    for cell in cells.iter() {
        let id_opt: Option<String> = match cell {
            Cell::Code(inner) => inner.extra.get("id").and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            Cell::Markdown(inner) => inner.extra.get("id").and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            Cell::Raw(inner) => inner.extra.get("id").and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
            Cell::Unknown(inner) => inner.value.get("id").and_then(|v| {
                if let Value::String(s) = v {
                    Some(s.clone())
                } else {
                    None
                }
            }),
        };
        if let Some(s) = id_opt {
            all_strings.insert(s);
        }
    }

    let mut seen: HashSet<String> = HashSet::new();
    let mut counter: u32 = 1;
    for cell in cells.iter_mut() {
        let current = file_id(cell).map(str::to_owned);
        let keep = matches!(current, Some(ref s) if !s.is_empty() && !seen.contains(s));
        if keep {
            let owned = current.expect("checked");
            *mem_id_mut(cell) = owned.clone();
            seen.insert(owned);
            continue;
        }
        loop {
            let candidate = format!("cell-{counter}");
            if !all_strings.contains(&candidate) && !seen.contains(&candidate) {
                *mem_id_mut(cell) = candidate.clone();
                seen.insert(candidate);
                counter += 1;
                break;
            }
            counter += 1;
        }
    }
}

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
/// CRLF resolved to LF; lone carriage returns are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultilineText {
    raw: MultilineRepr,
    /// Normalized reading text, computed on deserialization.
    normalized: String,
}

impl MultilineText {
    /// Normalized reading text with CRLF resolved to LF.
    ///
    /// Single carriage returns stay as-is.
    ///
    /// ```
    /// let text: europa_core::MultilineText =
    ///     serde_json::from_value(serde_json::json!(["a\r\n", "b"])).expect("loads");
    /// assert_eq!(text.text(), "a\nb");
    /// ```
    #[must_use]
    pub fn text(&self) -> &str {
        &self.normalized
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
        let (raw, joined) = match helper {
            Helper::Single(s) => (MultilineRepr::Single(s.clone()), s),
            Helper::Multiple(parts) => (MultilineRepr::Multiple(parts.clone()), parts.concat()),
        };
        let normalized = joined.replace("\r\n", "\n");
        Ok(Self { raw, normalized })
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
                normalized: "hello\nworld".to_owned(),
            }
        );

        let multi = roundtrip(serde_json::json!(["hello\n", "world"]));
        assert_eq!(
            multi,
            MultilineText {
                raw: MultilineRepr::Multiple(vec!["hello\n".to_owned(), "world".to_owned()]),
                normalized: "hello\nworld".to_owned(),
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
