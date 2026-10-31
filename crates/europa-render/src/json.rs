//! Pretty printing of JSON values for terminal display.
//!
//! Numbers keep the literal text they were read with and strings use an
//! escape of their own, so no control character reaches the display lines.

use europa_core::{Color, JsonValue, Style, StyledSpan};

use crate::blocks::Line;

/// Key style: blue and bold, close to the palette of familiar JSON tools.
fn key_style() -> Style {
    Style {
        fg: Some(Color::Indexed(4)),
        bold: true,
        ..Style::default()
    }
}

/// String style: green.
fn string_style() -> Style {
    Style {
        fg: Some(Color::Indexed(2)),
        ..Style::default()
    }
}

/// Number style: cyan.
fn number_style() -> Style {
    Style {
        fg: Some(Color::Indexed(6)),
        ..Style::default()
    }
}

/// Style of `true`, `false` and `null`: magenta.
fn keyword_style() -> Style {
    Style {
        fg: Some(Color::Indexed(5)),
        ..Style::default()
    }
}

/// Render a JSON value as indented logical lines.
///
/// One value or one key and value pair becomes one line. Nesting adds two
/// blanks per depth. Empty objects and arrays stay on one line.
#[allow(dead_code)]
pub(crate) fn render(value: &JsonValue) -> Vec<Line> {
    render_value(value, 0)
}

/// Indentation of `depth` levels as a single plain span.
fn indent(depth: usize) -> Vec<StyledSpan> {
    if depth == 0 {
        return Vec::new();
    }
    vec![StyledSpan {
        text: "  ".repeat(depth),
        style: Style::default(),
    }]
}

/// One logical line with the indentation of `depth`.
fn line_with(depth: usize, spans: Vec<StyledSpan>) -> Line {
    let lead = indent(depth);
    Line {
        hang: lead.clone(),
        lead,
        spans,
        fill: None,
    }
}

/// Plain text span.
fn plain(text: &str) -> StyledSpan {
    StyledSpan {
        text: text.to_owned(),
        style: Style::default(),
    }
}

/// Render a standalone value at the given depth.
fn render_value(value: &JsonValue, depth: usize) -> Vec<Line> {
    match value {
        JsonValue::Null => vec![line_with(depth, vec![styled("null", keyword_style())])],
        JsonValue::Bool(true) => vec![line_with(depth, vec![styled("true", keyword_style())])],
        JsonValue::Bool(false) => vec![line_with(depth, vec![styled("false", keyword_style())])],
        JsonValue::Number(number) => {
            vec![line_with(
                depth,
                vec![styled(number.as_str(), number_style())],
            )]
        }
        JsonValue::String(text) => {
            vec![line_with(depth, vec![styled(&quote(text), string_style())])]
        }
        JsonValue::Array(items) if items.is_empty() => {
            vec![line_with(depth, vec![plain("[]")])]
        }
        JsonValue::Object(map) if map.is_empty() => {
            vec![line_with(depth, vec![plain("{}")])]
        }
        JsonValue::Array(items) => {
            let mut lines = vec![line_with(depth, vec![plain("[")])];
            let last = items.len() - 1;
            for (index, item) in items.iter().enumerate() {
                let mut child = render_value(item, depth + 1);
                if index != last {
                    push_comma(child.last_mut().expect("child has a line"));
                }
                lines.extend(child);
            }
            lines.push(line_with(depth, vec![plain("]")]));
            lines
        }
        JsonValue::Object(map) => {
            let mut lines = vec![line_with(depth, vec![plain("{")])];
            let last = map.len() - 1;
            for (index, (key, item)) in map.iter().enumerate() {
                let mut child = render_pair(key, item, depth + 1);
                if index != last {
                    push_comma(child.last_mut().expect("pair has a line"));
                }
                lines.extend(child);
            }
            lines.push(line_with(depth, vec![plain("}")]));
            lines
        }
    }
}

/// Render one key and value pair at the given depth.
fn render_pair(key: &str, value: &JsonValue, depth: usize) -> Vec<Line> {
    let mut head = vec![styled(&quote(key), key_style()), plain(": ")];
    match value {
        JsonValue::Null => {
            head.push(styled("null", keyword_style()));
            vec![line_with(depth, head)]
        }
        JsonValue::Bool(true) => {
            head.push(styled("true", keyword_style()));
            vec![line_with(depth, head)]
        }
        JsonValue::Bool(false) => {
            head.push(styled("false", keyword_style()));
            vec![line_with(depth, head)]
        }
        JsonValue::Number(number) => {
            head.push(styled(number.as_str(), number_style()));
            vec![line_with(depth, head)]
        }
        JsonValue::String(text) => {
            head.push(styled(&quote(text), string_style()));
            vec![line_with(depth, head)]
        }
        JsonValue::Array(items) if items.is_empty() => {
            head.push(plain("[]"));
            vec![line_with(depth, head)]
        }
        JsonValue::Object(map) if map.is_empty() => {
            head.push(plain("{}"));
            vec![line_with(depth, head)]
        }
        JsonValue::Array(items) => {
            head.push(plain("["));
            let mut lines = vec![line_with(depth, head)];
            let last = items.len() - 1;
            for (index, item) in items.iter().enumerate() {
                let mut child = render_value(item, depth + 1);
                if index != last {
                    push_comma(child.last_mut().expect("child has a line"));
                }
                lines.extend(child);
            }
            lines.push(line_with(depth, vec![plain("]")]));
            lines
        }
        JsonValue::Object(map) => {
            head.push(plain("{"));
            let mut lines = vec![line_with(depth, head)];
            let last = map.len() - 1;
            for (index, (key, item)) in map.iter().enumerate() {
                let mut child = render_pair(key, item, depth + 1);
                if index != last {
                    push_comma(child.last_mut().expect("pair has a line"));
                }
                lines.extend(child);
            }
            lines.push(line_with(depth, vec![plain("}")]));
            lines
        }
    }
}

/// Styled span with the given text.
fn styled(text: &str, style: Style) -> StyledSpan {
    StyledSpan {
        text: text.to_owned(),
        style,
    }
}

/// Append a comma to the end of a logical line.
fn push_comma(line: &mut Line) {
    match line.spans.last_mut() {
        Some(last) if last.style == Style::default() => last.text.push(','),
        _ => line.spans.push(plain(",")),
    }
}

/// Quote a string the way JSON writes it.
///
/// The escape is written here because the generic one keeps DEL and C1
/// controls as they are, which must not reach the display lines.
fn quote(text: &str) -> String {
    let mut out = String::with_capacity(text.len() + 2);
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if c.is_control() => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}
