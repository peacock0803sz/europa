//! Notebook parsing and serialization entry points.

use std::collections::HashSet;

use serde::Serialize;
use serde_json::Value;

use crate::error::ParseError;
use crate::notebook::Notebook;

fn line_col(prefix: &[u8]) -> (usize, usize) {
    let mut line = 1_usize;
    let mut col_start = 0_usize;
    for (idx, byte) in prefix.iter().enumerate() {
        if *byte == b'\n' {
            line += 1;
            col_start = idx + 1;
        }
    }
    (line, prefix.len() - col_start + 1)
}

fn line_col_str(text: &str, byte_idx: usize) -> (usize, usize) {
    let prefix = &text.as_bytes()[..byte_idx.min(text.len())];
    line_col(prefix)
}

fn is_hex(byte: u8) -> bool {
    byte.is_ascii_hexdigit()
}

fn hex_val(byte: u8) -> u32 {
    match byte {
        b'0'..=b'9' => (byte - b'0') as u32,
        b'a'..=b'f' => (byte - b'a' + 10) as u32,
        b'A'..=b'F' => (byte - b'A' + 10) as u32,
        _ => 0,
    }
}

fn find_lone_surrogate(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut idx = 0_usize;
    let mut pending_high: Option<usize> = None;
    while idx < bytes.len() {
        if bytes[idx] != b'\\' {
            if pending_high.is_some() {
                return pending_high;
            }
            idx += 1;
            continue;
        }
        if idx + 1 >= bytes.len() {
            break;
        }
        let next = bytes[idx + 1];
        if next == b'\\' || next == b'"' || next == b'/' {
            if pending_high.is_some() {
                return pending_high;
            }
            idx += 2;
            continue;
        }
        if next != b'u' && next != b'U' {
            if pending_high.is_some() {
                return pending_high;
            }
            idx += 2;
            continue;
        }
        if idx + 5 >= bytes.len() {
            break;
        }
        let h0 = bytes[idx + 2];
        let h1 = bytes[idx + 3];
        let h2 = bytes[idx + 4];
        let h3 = bytes[idx + 5];
        if !(is_hex(h0) && is_hex(h1) && is_hex(h2) && is_hex(h3)) {
            if pending_high.is_some() {
                return pending_high;
            }
            idx += 2;
            continue;
        }
        let code = (hex_val(h0) << 12) | (hex_val(h1) << 8) | (hex_val(h2) << 4) | hex_val(h3);
        let start = idx;
        idx += 6;
        if (0xD800..0xDC00).contains(&code) {
            if pending_high.is_some() {
                return pending_high;
            }
            pending_high = Some(start);
        } else if (0xDC00..0xE000).contains(&code) {
            if pending_high.is_none() {
                return Some(start);
            }
            pending_high = None;
        } else if pending_high.is_some() {
            return pending_high;
        }
    }
    pending_high
}

fn find_duplicate_key(text: &str) -> Option<usize> {
    #[derive(PartialEq)]
    enum Ctx {
        Object {
            keys: HashSet<String>,
            expect_key: bool,
        },
        Array,
    }

    let bytes = text.as_bytes();
    let mut idx = 0_usize;
    let mut stack: Vec<Ctx> = Vec::new();
    let mut expect_value = false;

    let skip_ws = |mut i: usize| -> usize {
        while i < bytes.len() && matches!(bytes[i], b' ' | b'\t' | b'\n' | b'\r') {
            i += 1;
        }
        i
    };

    // Find the closing quote, then decode the raw literal with serde_json so
    // escaped and raw spellings of the same key compare equal.
    let parse_string = |mut i: usize| -> Option<(String, usize, usize)> {
        if i >= bytes.len() || bytes[i] != b'"' {
            return None;
        }
        let start = i;
        i += 1;
        while i < bytes.len() {
            match bytes[i] {
                b'"' => {
                    let decoded = serde_json::from_str::<String>(&text[start..=i]).ok()?;
                    return Some((decoded, start, i + 1));
                }
                b'\\' => i += 2,
                _ => i += 1,
            }
        }
        None
    };

    idx = skip_ws(idx);
    loop {
        idx = skip_ws(idx);
        if idx >= bytes.len() {
            return None;
        }
        let in_object_key_pos = matches!(
            stack.last(),
            Some(Ctx::Object {
                expect_key: true,
                ..
            })
        ) && !expect_value;
        match bytes[idx] {
            b'{' => {
                stack.push(Ctx::Object {
                    keys: HashSet::new(),
                    expect_key: true,
                });
                expect_value = false;
                idx += 1;
            }
            b'}' => {
                match stack.pop() {
                    Some(Ctx::Object { .. }) => {}
                    _ => return None,
                }
                expect_value = false;
                idx += 1;
            }
            b'[' => {
                stack.push(Ctx::Array);
                expect_value = false;
                idx += 1;
            }
            b']' => {
                match stack.pop() {
                    Some(Ctx::Array) => {}
                    _ => return None,
                }
                expect_value = false;
                idx += 1;
            }
            b'"' => {
                let (s, start, next) = parse_string(idx)?;
                if in_object_key_pos {
                    if let Some(Ctx::Object { keys, .. }) = stack.last_mut() {
                        if !keys.insert(s) {
                            return Some(start);
                        }
                    }
                    idx = skip_ws(next);
                    if idx < bytes.len() && bytes[idx] == b':' {
                        idx += 1;
                        expect_value = true;
                    } else {
                        return None;
                    }
                } else {
                    idx = next;
                    expect_value = false;
                    if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                        *expect_key = false;
                    }
                }
            }
            b':' | b',' => {
                idx += 1;
                if bytes[idx - 1] == b',' {
                    if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                        *expect_key = true;
                    }
                    expect_value = false;
                }
            }
            b't' => {
                if text[idx..].starts_with("true") {
                    idx += 4;
                    expect_value = false;
                    if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                        *expect_key = false;
                    }
                } else {
                    return None;
                }
            }
            b'f' => {
                if text[idx..].starts_with("false") {
                    idx += 5;
                    expect_value = false;
                    if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                        *expect_key = false;
                    }
                } else {
                    return None;
                }
            }
            b'n' => {
                if text[idx..].starts_with("null") {
                    idx += 4;
                    expect_value = false;
                    if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                        *expect_key = false;
                    }
                } else {
                    return None;
                }
            }
            b'-' | b'0'..=b'9' => {
                idx += 1;
                while idx < bytes.len()
                    && matches!(bytes[idx], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    idx += 1;
                }
                expect_value = false;
                if let Some(Ctx::Object { expect_key, .. }) = stack.last_mut() {
                    *expect_key = false;
                }
            }
            _ => return None,
        }
        if stack.is_empty() {
            return None;
        }
    }
}

fn is_string_or_string_array(value: &Value) -> bool {
    match value {
        Value::String(_) => true,
        Value::Array(parts) => parts.iter().all(|p| p.is_string()),
        _ => false,
    }
}

fn is_i64_range(value: &Value) -> bool {
    match value {
        Value::Null => true,
        Value::Number(n) => {
            if let Some(v) = n.as_i64() {
                let _ = v;
                true
            } else if let Some(v) = n.as_u64() {
                v <= i64::MAX as u64
            } else {
                false
            }
        }
        _ => false,
    }
}

fn mismatch(path: &str, message: &str) -> ParseError {
    ParseError::Mismatch {
        path: path.to_owned(),
        message: message.to_owned(),
    }
}

/// Parse notebook bytes into a notebook model.
///
/// Validates format conditions and rejects unsupported input with a reason.
#[allow(clippy::missing_panics_doc)]
#[allow(clippy::too_many_lines)]
pub fn parse(bytes: &[u8]) -> Result<Notebook, ParseError> {
    if bytes.len() >= 3 && bytes[0] == 0xEF && bytes[1] == 0xBB && bytes[2] == 0xBF {
        return Err(ParseError::InvalidJson {
            line: 1,
            column: 1,
            message: "byte order mark is not allowed".to_owned(),
        });
    }
    let text = match std::str::from_utf8(bytes) {
        Ok(valid) => valid,
        Err(error) => {
            let (line, column) = line_col(&bytes[..error.valid_up_to()]);
            return Err(ParseError::InvalidJson {
                line,
                column,
                message: "invalid UTF-8 sequence".to_owned(),
            });
        }
    };
    if let Some(byte_idx) = find_lone_surrogate(text) {
        let (line, column) = line_col_str(text, byte_idx);
        return Err(ParseError::InvalidJson {
            line,
            column,
            message: "unpaired surrogate escape".to_owned(),
        });
    }
    if let Some(byte_idx) = find_duplicate_key(text) {
        let (line, column) = line_col_str(text, byte_idx);
        return Err(ParseError::InvalidJson {
            line,
            column,
            message: "duplicate object key".to_owned(),
        });
    }
    let value: Value = match serde_json::from_slice(bytes) {
        Ok(parsed) => parsed,
        Err(error) => {
            return Err(ParseError::InvalidJson {
                line: error.line(),
                column: error.column(),
                message: error.to_string(),
            });
        }
    };
    let root = match value.as_object() {
        Some(map) => map,
        None => {
            return Err(mismatch("", "root must be an object"));
        }
    };
    match root.get("nbformat") {
        Some(Value::Number(n)) if n.as_i64() == Some(4) || n.as_u64() == Some(4) => {}
        _ => {
            return Err(ParseError::InvalidNbformat {
                path: "/nbformat".to_owned(),
                message: "nbformat must be integer 4".to_owned(),
            });
        }
    }
    match root.get("nbformat_minor") {
        Some(Value::Number(n)) => {
            let ok = if let Some(v) = n.as_i64() {
                v >= 0 && v <= u32::MAX as i64
            } else if let Some(v) = n.as_u64() {
                v <= u32::MAX as u64
            } else {
                false
            };
            if !ok {
                return Err(ParseError::InvalidNbformatMinor {
                    path: "/nbformat_minor".to_owned(),
                    message: "nbformat_minor must be a u32 integer".to_owned(),
                });
            }
        }
        _ => {
            return Err(ParseError::InvalidNbformatMinor {
                path: "/nbformat_minor".to_owned(),
                message: "nbformat_minor must be a u32 integer".to_owned(),
            });
        }
    }
    match root.get("metadata") {
        Some(Value::Object(_)) => {}
        _ => return Err(mismatch("/metadata", "metadata must be an object")),
    }
    let cells = match root.get("cells") {
        Some(Value::Array(items)) => items,
        _ => return Err(mismatch("/cells", "cells must be an array")),
    };
    for (cell_idx, cell) in cells.iter().enumerate() {
        let base = format!("/cells/{cell_idx}");
        let map = match cell.as_object() {
            Some(inner) => inner,
            None => return Err(mismatch(&base, "cell must be an object")),
        };
        let cell_type = match map.get("cell_type") {
            Some(Value::String(kind)) => kind.clone(),
            _ => {
                return Err(mismatch(
                    &format!("{base}/cell_type"),
                    "cell_type must be a string",
                ));
            }
        };
        if !["code", "markdown", "raw"].contains(&cell_type.as_str()) {
            continue;
        }
        match map.get("source") {
            Some(value) if is_string_or_string_array(value) => {}
            _ => {
                return Err(mismatch(
                    &format!("{base}/source"),
                    "source must be a string or an array of strings",
                ));
            }
        }
        match map.get("metadata") {
            Some(Value::Object(_)) => {}
            _ => {
                return Err(mismatch(
                    &format!("{base}/metadata"),
                    "cell metadata must be an object",
                ));
            }
        }
        if cell_type != "code" {
            continue;
        }
        let outputs = match map.get("outputs") {
            Some(Value::Array(items)) => items,
            _ => {
                return Err(mismatch(
                    &format!("{base}/outputs"),
                    "outputs must be an array",
                ));
            }
        };
        match map.get("execution_count") {
            Some(value) if is_i64_range(value) => {}
            _ => {
                return Err(mismatch(
                    &format!("{base}/execution_count"),
                    "execution_count must be an i64 integer or null",
                ));
            }
        }
        for (out_idx, output) in outputs.iter().enumerate() {
            let out_base = format!("{base}/outputs/{out_idx}");
            let out_map = match output.as_object() {
                Some(inner) => inner,
                None => return Err(mismatch(&out_base, "output must be an object")),
            };
            let output_type = match out_map.get("output_type") {
                Some(Value::String(kind)) => kind.clone(),
                _ => {
                    return Err(mismatch(
                        &format!("{out_base}/output_type"),
                        "output_type must be a string",
                    ));
                }
            };
            match output_type.as_str() {
                "stream" => {
                    match out_map.get("name") {
                        Some(Value::String(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/name"),
                                "stream name must be a string",
                            ));
                        }
                    }
                    match out_map.get("text") {
                        Some(value) if is_string_or_string_array(value) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/text"),
                                "stream text must be a string or an array of strings",
                            ));
                        }
                    }
                }
                "display_data" => {
                    match out_map.get("data") {
                        Some(Value::Object(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/data"),
                                "data must be an object",
                            ));
                        }
                    }
                    match out_map.get("metadata") {
                        Some(Value::Object(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/metadata"),
                                "output metadata must be an object",
                            ));
                        }
                    }
                }
                "execute_result" => {
                    match out_map.get("data") {
                        Some(Value::Object(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/data"),
                                "data must be an object",
                            ));
                        }
                    }
                    match out_map.get("metadata") {
                        Some(Value::Object(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/metadata"),
                                "output metadata must be an object",
                            ));
                        }
                    }
                    match out_map.get("execution_count") {
                        Some(value) if is_i64_range(value) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/execution_count"),
                                "execution_count must be an i64 integer or null",
                            ));
                        }
                    }
                }
                "error" => {
                    match out_map.get("ename") {
                        Some(Value::String(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/ename"),
                                "ename must be a string",
                            ));
                        }
                    }
                    match out_map.get("evalue") {
                        Some(Value::String(_)) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/evalue"),
                                "evalue must be a string",
                            ));
                        }
                    }
                    match out_map.get("traceback") {
                        Some(Value::Array(lines)) if lines.iter().all(|line| line.is_string()) => {}
                        _ => {
                            return Err(mismatch(
                                &format!("{out_base}/traceback"),
                                "traceback must be an array of strings",
                            ));
                        }
                    }
                }
                _ => {}
            }
        }
    }
    let mut notebook: Notebook = serde_json::from_value(value)
        .map_err(|error| mismatch("", &format!("structural mismatch: {error}")))?;
    crate::normalize::assign_cell_ids(&mut notebook.cells);
    Ok(notebook)
}

/// Serialize a notebook model back to `.ipynb` text.
///
/// Keys sort by byte order at every level, which relies on the
/// `serde_json` `preserve_order` feature staying off. Output uses
/// one-space indentation and ends with a trailing newline.
#[allow(clippy::missing_panics_doc)]
pub fn serialize(notebook: &Notebook) -> String {
    let value = serde_json::to_value(notebook).expect("notebook serializes to JSON");
    let mut buffer = Vec::new();
    let formatter = serde_json::ser::PrettyFormatter::with_indent(b" ");
    let mut serializer = serde_json::ser::Serializer::with_formatter(&mut buffer, formatter);
    value
        .serialize(&mut serializer)
        .expect("JSON writes to memory");
    let mut text = String::from_utf8(buffer).expect("serializer emits UTF-8");
    if !text.ends_with('\n') {
        text.push('\n');
    }
    text
}
