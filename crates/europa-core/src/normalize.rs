//! Multiline text handling and cell identifier assignment.

use serde::{Deserialize, Serialize};

use crate::json::JsonValue;
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
/// writes back the same form. The exception is the text of a stream output
/// built with [`Output::new_stream`](crate::Output::new_stream) or of a loaded
/// one that [`apply_iopub`](crate::apply_iopub) appended to, which saving
/// splits into lines; the output writer does that, not this type. Reading
/// uses the normalized text with CRLF resolved to LF; lone carriage returns
/// are kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MultilineText {
    raw: MultilineRepr,
    /// Normalized reading text, computed whenever a value is built.
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

impl MultilineText {
    fn from_repr(raw: MultilineRepr) -> MultilineText {
        let joined = match &raw {
            MultilineRepr::Single(text) => text.clone(),
            MultilineRepr::Multiple(parts) => parts.concat(),
        };
        let normalized = joined.replace("\r\n", "\n");
        MultilineText { raw, normalized }
    }

    /// Rewrite the text held as a single string in place.
    ///
    /// Returns `false` and leaves the text alone when it is held as an array
    /// of strings. `f` must keep everything up to and including the last `\n`
    /// present before the call; only the text after it is renormalized, since
    /// renormalizing the whole text would scan it on every call.
    pub(crate) fn edit_tail(&mut self, f: impl FnOnce(&mut String)) -> bool {
        let MultilineRepr::Single(text) = &mut self.raw else {
            return false;
        };
        let raw_cut = text.rfind('\n').map_or(0, |at| at + 1);
        let normalized_cut = self.normalized.rfind('\n').map_or(0, |at| at + 1);
        f(text);
        debug_assert!(
            text.len() >= raw_cut && (raw_cut == 0 || text.as_bytes()[raw_cut - 1] == b'\n'),
            "edit_tail must keep everything up to the last newline"
        );
        self.normalized.truncate(normalized_cut);
        self.normalized
            .push_str(&text[raw_cut..].replace("\r\n", "\n"));
        true
    }

    /// Append `tail` to the text held as a single string.
    ///
    /// Returns `false` and leaves the text alone when it is held as an array
    /// of strings. `tail` must hold no `\r` and the text must not end with
    /// `\r`, so no CRLF forms across the join and `tail` is already
    /// normalized.
    pub(crate) fn push_tail(&mut self, tail: &str) -> bool {
        let MultilineRepr::Single(text) = &mut self.raw else {
            return false;
        };
        debug_assert!(
            !tail.contains('\r') && !text.ends_with('\r'),
            "push_tail must not need renormalizing"
        );
        text.push_str(tail);
        self.normalized.push_str(tail);
        true
    }

    /// Keep `text` as one string.
    pub(crate) fn single(text: String) -> MultilineText {
        Self::from_repr(MultilineRepr::Single(text))
    }

    /// The original string, when the text is held as a single string.
    pub(crate) fn as_single(&self) -> Option<&str> {
        match &self.raw {
            MultilineRepr::Single(text) => Some(text),
            MultilineRepr::Multiple(_) => None,
        }
    }

    /// Read a string or an array of strings, keeping which form was used.
    pub(crate) fn from_json(value: &JsonValue) -> Option<MultilineText> {
        match value {
            JsonValue::String(text) => Some(Self::from_repr(MultilineRepr::Single(text.clone()))),
            JsonValue::Array(items) => items
                .iter()
                .map(|item| item.as_str().map(str::to_owned))
                .collect::<Option<Vec<_>>>()
                .map(|parts| Self::from_repr(MultilineRepr::Multiple(parts))),
            _ => None,
        }
    }

    /// Write the form held: one string or the array of strings.
    pub(crate) fn to_json(&self) -> JsonValue {
        match &self.raw {
            MultilineRepr::Single(text) => JsonValue::from(text.as_str()),
            MultilineRepr::Multiple(parts) => JsonValue::Array(
                parts
                    .iter()
                    .map(|part| JsonValue::from(part.as_str()))
                    .collect(),
            ),
        }
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
    fn push_tail_extends_raw_and_reading_text() {
        let mut text = MultilineText::single("a\r\nb".to_owned());
        assert!(text.push_tail("c"));
        assert_eq!(text.as_single(), Some("a\r\nbc"));
        assert_eq!(text.text(), "a\nbc");

        let mut array = roundtrip(serde_json::json!(["a"]));
        assert!(!array.push_tail("b"));
        assert_eq!(array.text(), "a");
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
