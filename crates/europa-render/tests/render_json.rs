mod common;

use europa_core::{Color, Style};
use europa_render::render_outputs;

fn render(data: &str) -> (europa_core::RenderedCell, Vec<String>) {
    let cell = render_outputs(&[common::display(data)], &common::opts(80));
    let texts = common::texts(&cell);
    common::assert_lines_are_well_formed(&cell);
    (cell, texts)
}

fn style_of(cell: &europa_core::RenderedCell, line: usize, col: usize) -> Style {
    let mut seen = 0;
    for span in &cell.lines[line].spans {
        let len = span.text.chars().count();
        if col < seen + len {
            return span.style;
        }
        seen += len;
    }
    panic!("column {col} out of line {line}");
}

#[test]
fn nested_object_renders_one_value_per_line() {
    let (_, texts) = render(
        r#"{"application/json": {"a": {"b": [1, 2, {"c": null}]}, "d": "text"}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert_eq!(
        texts,
        [
            "{",
            "  \"a\": {",
            "    \"b\": [",
            "      1,",
            "      2,",
            "      {",
            "        \"c\": null",
            "      }",
            "    ]",
            "  },",
            "  \"d\": \"text\"",
            "}",
        ]
    );
}

#[test]
fn object_keys_render_in_code_point_order() {
    let (_, texts) = render(
        r#"{"application/json": {"z": 1, "a": 2, "m": 3}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert_eq!(
        texts,
        ["{", "  \"a\": 2,", "  \"m\": 3,", "  \"z\": 1", "}",]
    );
}

#[test]
fn numbers_keep_their_written_form() {
    let (_, texts) = render(
        r#"{"application/json": [1.0, 1e3, 12345678901234567890], "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert!(texts.iter().any(|line| line.contains("1.0,")), "{texts:?}");
    assert!(texts.iter().any(|line| line.contains("1e3,")), "{texts:?}");
    assert!(
        texts
            .iter()
            .any(|line| line.contains("12345678901234567890")),
        "{texts:?}"
    );
}

#[test]
fn empty_object_and_array_stay_on_one_line() {
    let (_, texts) = render(
        r#"{"application/json": {"o": {}, "a": []}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert_eq!(texts, ["{", "  \"a\": [],", "  \"o\": {}", "}",]);
    let (_, empty_object) =
        render(r#"{"application/json": {}, "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(empty_object, ["{}"]);
    let (_, empty_array) =
        render(r#"{"application/json": [], "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(empty_array, ["[]"]);
}

#[test]
fn control_characters_render_as_escapes_on_one_line() {
    let (cell, texts) = render(
        r#"{"application/json": "a\nb\u001b[31m\u007f\u0085", "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert_eq!(texts.len(), 1);
    assert_eq!(texts[0], "\"a\\nb\\u001b[31m\\u007f\\u0085\"");
    for span in cell.lines[0].spans.iter() {
        assert!(!span.style.bold, "{span:?}");
        assert!(!span.style.dim, "{span:?}");
        assert!(!span.style.italic, "{span:?}");
        assert!(!span.style.underline, "{span:?}");
        assert!(!span.style.reverse, "{span:?}");
        assert!(!span.style.strikethrough, "{span:?}");
        assert_eq!(span.style.bg, None, "{span:?}");
    }
}

#[test]
fn single_values_render_on_one_line() {
    let (_, text) =
        render(r#"{"application/json": "s", "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(text, ["\"s\""]);
    let (_, number) =
        render(r#"{"application/json": 3, "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(number, ["3"]);
    let (_, truth) =
        render(r#"{"application/json": true, "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(truth, ["true"]);
    let (_, nothing) =
        render(r#"{"application/json": null, "text/plain": "<IPython.core.display.JSON object>"}"#);
    assert_eq!(nothing, ["null"]);
}

#[test]
fn values_have_their_own_colors() {
    let (cell, _) = render(
        r#"{"application/json": {"a": "s", "n": 3, "t": true}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    let key = Style {
        fg: Some(Color::Indexed(4)),
        bold: true,
        ..Style::default()
    };
    let string = Style {
        fg: Some(Color::Indexed(2)),
        ..Style::default()
    };
    let number = Style {
        fg: Some(Color::Indexed(6)),
        ..Style::default()
    };
    let keyword = Style {
        fg: Some(Color::Indexed(5)),
        ..Style::default()
    };
    let line = cell
        .lines
        .iter()
        .position(|line| line.spans.iter().any(|span| span.text.contains("\"a\"")))
        .expect("key line");
    let text: String = cell.lines[line]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    let key_col = text.find('a').expect("key");
    assert_eq!(style_of(&cell, line, key_col), key);
    let value_col = text.find('s').expect("string");
    assert_eq!(style_of(&cell, line, value_col), string);
    let number_line = cell
        .lines
        .iter()
        .position(|line| line.spans.iter().any(|span| span.text.contains("\"n\"")))
        .expect("number line");
    let number_text: String = cell.lines[number_line]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    let digit_col = number_text.find('3').expect("number");
    assert_eq!(style_of(&cell, number_line, digit_col), number);
    let keyword_line = cell
        .lines
        .iter()
        .position(|line| line.spans.iter().any(|span| span.text.contains("\"t\"")))
        .expect("keyword line");
    let keyword_text: String = cell.lines[keyword_line]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    let keyword_col = keyword_text.find("true").expect("keyword");
    assert_eq!(style_of(&cell, keyword_line, keyword_col), keyword);
    assert_eq!(style_of(&cell, keyword_line, 0), Style::default());
}

#[test]
fn deep_nesting_wraps_without_losing_characters() {
    let mut value = String::from("1");
    for _ in 0..50 {
        value = format!("[{value}]");
    }
    let data = format!(
        "{{\"application/json\": {value}, \"text/plain\": \"<IPython.core.display.JSON object>\"}}"
    );
    let cell = render_outputs(&[common::display(&data)], &common::limited(20, 1000));
    common::assert_lines_are_well_formed(&cell);
    let joined: String = common::texts(&cell)
        .join("")
        .chars()
        .filter(|c| !c.is_whitespace())
        .collect();
    let expected: String = value.chars().filter(|c| !c.is_whitespace()).collect();
    assert_eq!(joined, expected);
}

#[test]
fn indentation_and_separators_round_trip() {
    let (_, texts) = render(
        r#"{"application/json": {"a": {"b": [1, 2, {"c": null}]}, "d": "text"}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    let joined: String = texts
        .iter()
        .map(|line| line.trim_start())
        .collect::<Vec<_>>()
        .join("")
        .replace("\": \"", "\":\"")
        .replace("\": ", "\":");
    assert_eq!(joined, "{\"a\":{\"b\":[1,2,{\"c\":null}]},\"d\":\"text\"}");
}

#[test]
fn huge_array_stays_within_the_line_limit() {
    let mut items = Vec::new();
    for i in 0..100000 {
        items.push(i.to_string());
    }
    let data = format!(
        "{{\"application/json\": [{}], \"text/plain\": \"<IPython.core.display.JSON object>\"}}",
        items.join(", ")
    );
    let cell = render_outputs(&[common::display(&data)], &common::limited(80, 200));
    assert!(cell.lines.len() <= 200, "{}", cell.lines.len());
}

#[test]
fn wrapped_string_continuation_keeps_indentation() {
    let data = r#"{"application/json": {"key": "abcdefghijklmnopqrstuvwxyz0123456789"}, "text/plain": "<IPython.core.display.JSON object>"}"#;
    let cell = render_outputs(&[common::display(data)], &common::opts(20));
    common::assert_lines_are_well_formed(&cell);
    let texts = common::texts(&cell);
    assert!(texts.len() > 3, "{texts:?}");
    for line in texts.iter().skip(1).take(texts.len() - 2) {
        assert!(line.starts_with(' '), "{line:?}");
    }
}
