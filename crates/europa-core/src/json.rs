//! JSON value type that keeps number literals as written.
//!
//! [`JsonValue`] replaces `serde_json::Value` in the notebook model so that
//! numbers such as `1E5`, `1.50` or `18446744073709551616` can be written back
//! exactly as they were read.

// The bodies are placeholders until the implementation lands.
#![allow(unused_variables)]

use std::collections::BTreeMap;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// JSON number kept as its literal text.
///
/// Two numbers are equal only when their literals are equal, so `1.0` and
/// `1.00` differ. A literal always follows the RFC 8259 number grammar and
/// rounds to a finite `f64`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonNumber {
    literal: String,
}

impl JsonNumber {
    /// Build a number from its literal text.
    ///
    /// Accepts only literals that match the RFC 8259 number grammar and whose
    /// nearest `f64` is finite.
    #[must_use]
    pub fn from_literal(literal: &str) -> Option<JsonNumber> {
        todo!()
    }

    /// Literal text exactly as it was read or built.
    #[must_use]
    pub fn as_str(&self) -> &str {
        todo!()
    }

    /// Value as `i64`, only for integer literals in range.
    ///
    /// Literals with a fraction or an exponent, and `-0`, return `None`.
    #[must_use]
    pub fn as_i64(&self) -> Option<i64> {
        todo!()
    }

    /// Value as `u64`, only for integer literals in range.
    ///
    /// Literals with a fraction or an exponent, and `-0`, return `None`.
    #[must_use]
    pub fn as_u64(&self) -> Option<u64> {
        todo!()
    }

    /// Nearest `f64`, always finite.
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        todo!()
    }
}

impl From<i64> for JsonNumber {
    fn from(value: i64) -> Self {
        todo!()
    }
}

impl From<u64> for JsonNumber {
    fn from(value: u64) -> Self {
        todo!()
    }
}

impl From<u32> for JsonNumber {
    fn from(value: u32) -> Self {
        todo!()
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
        todo!()
    }

    /// Boolean, if this is a boolean.
    #[must_use]
    pub fn as_bool(&self) -> Option<bool> {
        todo!()
    }

    /// Number, if this is a number.
    #[must_use]
    pub fn as_number(&self) -> Option<&JsonNumber> {
        todo!()
    }

    /// Elements, if this is an array.
    #[must_use]
    pub fn as_array(&self) -> Option<&Vec<JsonValue>> {
        todo!()
    }

    /// Members, if this is an object.
    #[must_use]
    pub fn as_object(&self) -> Option<&JsonMap> {
        todo!()
    }

    /// Whether this is `null`.
    #[must_use]
    pub fn is_null(&self) -> bool {
        todo!()
    }
}

impl From<bool> for JsonValue {
    fn from(value: bool) -> Self {
        todo!()
    }
}

impl From<String> for JsonValue {
    fn from(value: String) -> Self {
        todo!()
    }
}

impl From<&str> for JsonValue {
    fn from(value: &str) -> Self {
        todo!()
    }
}

impl From<JsonNumber> for JsonValue {
    fn from(value: JsonNumber) -> Self {
        todo!()
    }
}

impl From<i64> for JsonValue {
    fn from(value: i64) -> Self {
        todo!()
    }
}

impl From<u64> for JsonValue {
    fn from(value: u64) -> Self {
        todo!()
    }
}

impl From<Vec<JsonValue>> for JsonValue {
    fn from(value: Vec<JsonValue>) -> Self {
        todo!()
    }
}

impl From<JsonMap> for JsonValue {
    fn from(value: JsonMap) -> Self {
        todo!()
    }
}

impl Serialize for JsonValue {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        todo!()
    }
}

impl<'de> Deserialize<'de> for JsonValue {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        todo!()
    }
}
