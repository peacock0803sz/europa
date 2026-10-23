//! JSON value type that keeps number literals as written.
//!
//! [`JsonValue`] replaces `serde_json::Value` in the notebook model so that
//! numbers such as `1E5`, `1.50` or `18446744073709551616` can be written back
//! exactly as they were read.

use std::collections::BTreeMap;
use std::fmt;

use serde::de::{self, MapAccess, SeqAccess, Visitor};
use serde::ser::{SerializeMap, SerializeSeq};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use crate::error::ParseError;

/// JSON number kept as its literal text.
///
/// Two numbers are equal only when their literals are equal, so `1.0` and
/// `1.00` differ. A literal always follows the RFC 8259 number grammar and
/// rounds to a finite `f64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonNumber {
    literal: String,
}

/// Whether `literal` matches `-?(0|[1-9][0-9]*)(\.[0-9]+)?([eE][+-]?[0-9]+)?`.
fn matches_number_grammar(literal: &str) -> bool {
    let bytes = literal.as_bytes();
    let digits = |from: usize| {
        bytes[from..]
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count()
    };
    let mut idx = usize::from(bytes.first() == Some(&b'-'));
    match bytes.get(idx) {
        Some(b'0') => idx += 1,
        Some(b'1'..=b'9') => idx += digits(idx),
        _ => return false,
    }
    if bytes.get(idx) == Some(&b'.') {
        let count = digits(idx + 1);
        if count == 0 {
            return false;
        }
        idx += 1 + count;
    }
    if matches!(bytes.get(idx), Some(b'e' | b'E')) {
        idx += 1;
        if matches!(bytes.get(idx), Some(b'+' | b'-')) {
            idx += 1;
        }
        let count = digits(idx);
        if count == 0 {
            return false;
        }
        idx += count;
    }
    idx == bytes.len()
}

impl JsonNumber {
    /// Build a number from its literal text.
    ///
    /// Accepts only literals that match the RFC 8259 number grammar and whose
    /// nearest `f64` is finite.
    #[must_use]
    pub fn from_literal(literal: &str) -> Option<JsonNumber> {
        if !matches_number_grammar(literal) {
            return None;
        }
        match literal.parse::<f64>() {
            Ok(rounded) if rounded.is_finite() => Some(Self::from_validated(literal)),
            _ => None,
        }
    }

    /// Keep a literal that is already known to be valid.
    pub(crate) fn from_validated(literal: &str) -> JsonNumber {
        JsonNumber {
            literal: literal.to_owned(),
        }
    }

    /// Literal text exactly as it was read or built.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.literal
    }

    fn is_plain_integer(&self) -> bool {
        self.literal != "-0" && !self.literal.contains(['.', 'e', 'E'])
    }

    /// Value as `i64`, only for integer literals in range.
    ///
    /// Literals with a fraction or an exponent, and `-0`, return `None`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        if self.is_plain_integer() {
            self.literal.parse().ok()
        } else {
            None
        }
    }

    /// Value as `u64`, only for integer literals in range.
    ///
    /// Literals with a fraction or an exponent, and `-0`, return `None`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        if self.is_plain_integer() {
            self.literal.parse().ok()
        } else {
            None
        }
    }

    /// Nearest `f64`, always finite.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        self.literal
            .parse()
            .expect("a literal always rounds to a finite f64")
    }
}

impl From<i64> for JsonNumber {
    fn from(value: i64) -> Self {
        Self::from_validated(&value.to_string())
    }
}

impl From<u64> for JsonNumber {
    fn from(value: u64) -> Self {
        Self::from_validated(&value.to_string())
    }
}

impl From<u32> for JsonNumber {
    fn from(value: u32) -> Self {
        Self::from_validated(&value.to_string())
    }
}

/// JSON object with keys in UTF-8 byte order.
///
/// The byte order of UTF-8 matches code point order, which is the order
/// nbformat writes keys in.
pub type JsonMap = BTreeMap<String, JsonValue>;

/// JSON value whose numbers keep their literal text.
///
/// Equality follows structure and number literals.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JsonValue {
    /// `null`.
    Null,
    /// `true` or `false`.
    Bool(bool),
    /// Number kept as its literal.
    Number(JsonNumber),
    /// String.
    String(String),
    /// Array.
    Array(Vec<JsonValue>),
    /// Object.
    Object(JsonMap),
}

impl JsonValue {
    /// String content, if this is a string.
    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(text) => Some(text),
            _ => None,
        }
    }

    /// Boolean, if this is a boolean.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        match self {
            Self::Bool(flag) => Some(*flag),
            _ => None,
        }
    }

    /// Number, if this is a number.
    #[must_use]
    pub fn as_number(&self) -> Option<&JsonNumber> {
        match self {
            Self::Number(number) => Some(number),
            _ => None,
        }
    }

    /// Elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    /// Members, if this is an object.
    #[must_use]
    pub fn as_object(&self) -> Option<&JsonMap> {
        match self {
            Self::Object(map) => Some(map),
            _ => None,
        }
    }

    /// Whether this is `null`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        matches!(self, Self::Null)
    }
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        Self::Bool(value)
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        Self::String(value.to_owned())
    }
}

impl From<JsonNumber> for JsonValue {
    fn from(value: JsonNumber) -> Self {
        Self::Number(value)
    }
}

impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        Self::Number(value.into())
    }
}

impl From<u64> for JsonValue {
    fn from(value: u64) -> Self {
        Self::Number(value.into())
    }
}

impl From<Vec<JsonValue>> for JsonValue {
    fn from(value: Vec<JsonValue>) -> Self {
        Self::Array(value)
    }
}

impl From<JsonMap> for JsonValue {
    fn from(value: JsonMap) -> Self {
        Self::Object(value)
    }
}

impl Serialize for JsonNumber {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        if let Some(value) = self.as_i64() {
            serializer.serialize_i64(value)
        } else if let Some(value) = self.as_u64() {
            serializer.serialize_u64(value)
        } else {
            serializer.serialize_f64(self.as_f64())
        }
    }
}

impl Serialize for JsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Bool(flag) => serializer.serialize_bool(*flag),
            Self::Number(number) => number.serialize(serializer),
            Self::String(text) => serializer.serialize_str(text),
            Self::Array(items) => {
                let mut seq = serializer.serialize_seq(Some(items.len()))?;
                for item in items {
                    seq.serialize_element(item)?;
                }
                seq.end()
            }
            Self::Object(map) => {
                let mut out = serializer.serialize_map(Some(map.len()))?;
                for (key, value) in map {
                    out.serialize_entry(key, value)?;
                }
                out.end()
            }
        }
    }
}

struct JsonValueVisitor;

impl<'de> Visitor<'de> for JsonValueVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("any JSON value")
    }

    fn visit_unit<E>(self) -> Result<JsonValue, E> {
        Ok(JsonValue::Null)
    }

    fn visit_none<E>(self) -> Result<JsonValue, E> {
        Ok(JsonValue::Null)
    }

    fn visit_bool<E>(self, value: bool) -> Result<JsonValue, E> {
        Ok(JsonValue::Bool(value))
    }

    fn visit_i64<E>(self, value: i64) -> Result<JsonValue, E> {
        Ok(value.into())
    }

    fn visit_u64<E>(self, value: u64) -> Result<JsonValue, E> {
        Ok(value.into())
    }

    fn visit_f64<E>(self, value: f64) -> Result<JsonValue, E>
    where
        E: de::Error,
    {
        let number = serde_json::Number::from_f64(value)
            .ok_or_else(|| E::custom("non-finite number is not valid JSON"))?;
        JsonNumber::from_literal(&number.to_string())
            .map(JsonValue::Number)
            .ok_or_else(|| E::custom("number is out of range"))
    }

    fn visit_str<E>(self, value: &str) -> Result<JsonValue, E> {
        Ok(value.into())
    }

    fn visit_string<E>(self, value: String) -> Result<JsonValue, E> {
        Ok(JsonValue::String(value))
    }

    fn visit_some<D>(self, deserializer: D) -> Result<JsonValue, D::Error>
    where
        D: Deserializer<'de>,
    {
        JsonValue::deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<JsonValue, A::Error>
    where
        A: SeqAccess<'de>,
    {
        let mut items = Vec::with_capacity(seq.size_hint().unwrap_or(0).min(4096));
        while let Some(item) = seq.next_element()? {
            items.push(item);
        }
        Ok(JsonValue::Array(items))
    }

    fn visit_map<A>(self, mut access: A) -> Result<JsonValue, A::Error>
    where
        A: MapAccess<'de>,
    {
        let mut map = JsonMap::new();
        while let Some((key, value)) = access.next_entry()? {
            map.insert(key, value);
        }
        Ok(JsonValue::Object(map))
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        deserializer.deserialize_any(JsonValueVisitor)
    }
}

/// Build a value tree from text that `serde_json` already accepted.
///
/// Keeps number literals unchanged. Never panics: input that does not follow
/// the grammar yields [`ParseError::Mismatch`] with an empty path.
#[allow(dead_code, unused_variables)]
pub(crate) fn read_validated(text: &str) -> Result<JsonValue, ParseError> {
    todo!()
}

/// Write a value in the layout nbformat uses for `.ipynb` files.
///
/// One-space indent, keys in code point order, `": "` and `,` separators,
/// non-ASCII text unescaped and a single trailing newline.
#[allow(dead_code, unused_variables)]
pub(crate) fn write_ipynb(value: &JsonValue) -> String {
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn num(literal: &str) -> JsonNumber {
        JsonNumber::from_literal(literal).expect("valid literal")
    }

    #[test]
    fn from_literal_accepts_grammar_and_finite_values() {
        for literal in [
            "0",
            "-0",
            "1.50",
            "1E5",
            "1e-400",
            "18446744073709551616",
            "1.7976931348623157e308",
            "1.7976931348623158e308",
            "-1.7976931348623157e308",
            "-1.7976931348623158e308",
        ] {
            let number = JsonNumber::from_literal(literal);
            assert_eq!(number.as_ref().map(JsonNumber::as_str), Some(literal));
        }
    }

    #[test]
    fn from_literal_rejects_bad_grammar_and_overflow() {
        for literal in [
            "01",
            "+1",
            ".5",
            "1.",
            "1e",
            "-",
            "NaN",
            "Infinity",
            "1e400",
            "1.7976931348623159e308",
            "-1.7976931348623159e308",
            "",
        ] {
            assert!(
                JsonNumber::from_literal(literal).is_none(),
                "{literal:?} must be rejected"
            );
        }
    }

    #[test]
    fn from_literal_agrees_with_parse_near_f64_max() {
        for literal in [
            "1.7976931348623157e308",
            "1.7976931348623158e308",
            "1.7976931348623159e308",
        ] {
            for sign in ["", "-"] {
                let literal = format!("{sign}{literal}");
                let text = format!(
                    r#"{{"nbformat":4,"nbformat_minor":5,"metadata":{{"x":{literal}}},"cells":[]}}"#
                );
                assert_eq!(
                    crate::parse(text.as_bytes()).is_ok(),
                    JsonNumber::from_literal(&literal).is_some(),
                    "{literal}"
                );
            }
        }
    }

    #[test]
    fn integer_accessors_accept_only_plain_integers() {
        assert_eq!(num("4").as_i64(), Some(4));
        assert_eq!(num("4").as_u64(), Some(4));
        assert_eq!(num("-0").as_i64(), None);
        assert_eq!(num("-0").as_u64(), None);
        assert_eq!(num("4.0").as_i64(), None);
        assert_eq!(num("4e0").as_u64(), None);
        assert_eq!(num("9223372036854775808").as_i64(), None);
        assert_eq!(
            num("9223372036854775808").as_u64(),
            Some(9_223_372_036_854_775_808)
        );
        assert_eq!(num("18446744073709551616").as_u64(), None);
        assert_eq!(num("-3").as_u64(), None);
    }

    #[test]
    fn as_f64_rounds_to_nearest() {
        assert!((num("1.5").as_f64() - 1.5).abs() < f64::EPSILON);
        assert!(num("-0").as_f64().is_sign_negative());
    }

    #[test]
    fn from_integers_build_literals() {
        assert_eq!(JsonNumber::from(-3_i64).as_str(), "-3");
        assert_eq!(JsonNumber::from(5_u32).as_str(), "5");
        assert_eq!(JsonNumber::from(u64::MAX).as_str(), "18446744073709551615");
    }

    #[test]
    fn equality_follows_literals() {
        assert_ne!(num("1.0"), num("1.00"));
        assert_eq!(num("1.0"), num("1.0"));
    }

    #[test]
    fn value_accessors_and_conversions() {
        assert_eq!(JsonValue::from("a").as_str(), Some("a"));
        assert_eq!(JsonValue::from(true).as_bool(), Some(true));
        assert_eq!(
            JsonValue::from(7_i64).as_number().map(JsonNumber::as_str),
            Some("7")
        );
        assert!(JsonValue::Null.is_null());
        assert!(JsonValue::from(vec![JsonValue::Null]).as_array().is_some());
        assert!(JsonValue::from(JsonMap::new()).as_object().is_some());
        assert_eq!(JsonValue::from(1_u64).as_str(), None);
    }

    #[test]
    fn serde_writes_integers_and_floats() {
        let value = JsonValue::Array(vec![
            JsonValue::from(1_i64),
            JsonValue::from(num("1.5")),
            JsonValue::from(num("1E2")),
        ]);
        assert_eq!(
            serde_json::to_string(&value).expect("serializes"),
            "[1,1.5,100.0]"
        );
    }

    #[test]
    fn serde_reads_numbers_with_their_spelling() {
        let value: JsonValue =
            serde_json::from_str("[1, -2, 1.5, 18446744073709551615]").expect("deserializes");
        let literals: Vec<&str> = value
            .as_array()
            .expect("array")
            .iter()
            .map(|item| item.as_number().expect("number").as_str())
            .collect();
        assert_eq!(literals, ["1", "-2", "1.5", "18446744073709551615"]);
    }
}
