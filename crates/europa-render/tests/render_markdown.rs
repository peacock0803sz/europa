mod common;

use europa_core::Style;
use europa_render::render_outputs;

fn show(markdown: &str) -> (europa_core::RenderedCell, Vec<String>) {
    let data = serde_json::to_string(&serde_json::json!({
        "text/markdown": markdown,
        "text/plain": "<IPython.core.display.Markdown object>"
    }))
    .expect("data");
    render(&data)
}

fn render(data: &str) -> (europa_core::RenderedCell, Vec<String>) {
    let cell = render_outputs(&[common::display(data)], &common::opts(80));
    let texts = common::texts(&cell);
    common::assert_lines_are_well_formed(&cell);
    (cell, texts)
}

fn show_with(
    markdown: &str,
    options: &europa_core::RenderOptions,
) -> (europa_core::RenderedCell, Vec<String>) {
    let data = serde_json::to_string(&serde_json::json!({
        "text/markdown": markdown,
        "text/plain": "<IPython.core.display.Markdown object>"
    }))
    .expect("data");
    let cell = render_outputs(&[common::display(&data)], options);
    let texts = common::texts(&cell);
    common::assert_lines_are_well_formed(&cell);
    (cell, texts)
}

fn style_at(cell: &europa_core::RenderedCell, line: usize, col: usize) -> Style {
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
fn headings_and_emphasis_have_styles() {
    let (cell, texts) = show("# Title\n\nSome **bold** and *italic* text.");
    assert_eq!(texts, ["Title", "", "Some bold and italic text."]);
    let title = style_at(&cell, 0, 0);
    assert!(title.bold && title.underline, "{title:?}");
    let body: String = cell.lines[2]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    let bold_col = body.find("bold").expect("bold");
    assert!(style_at(&cell, 2, bold_col).bold);
    let italic_col = body.find("italic").expect("italic");
    assert!(style_at(&cell, 2, italic_col).italic);
    let (cell, _) = show("## A");
    assert!(style_at(&cell, 0, 0).bold);
    for level in ["### A", "#### A", "##### A", "###### A"] {
        let (cell, _) = show(level);
        let style = style_at(&cell, 0, 0);
        assert!(style.bold && style.dim, "{level} -> {style:?}");
    }
    let (cell, texts) = show("~~x~~");
    assert_eq!(texts, ["x"]);
    assert!(style_at(&cell, 0, 0).strikethrough);
}

#[test]
fn soft_breaks_join_and_hard_breaks_split() {
    let (_, texts) = show("one\ntwo\n\nthree  \nfour\n\nfive\\\nsix");
    assert_eq!(texts, ["one two", "", "three", "four", "", "five", "six"]);
}

#[test]
fn math_footnote_and_task_marks_stay_as_written() {
    let (_, texts) = show("$x^2$ and $$y$$");
    assert_eq!(texts, ["$x^2$ and $$y$$"]);
    let (_, texts) = show("[^1] stays");
    assert_eq!(texts, ["[^1] stays"]);
    let (_, texts) = show("- [ ] a");
    assert!(texts.concat().contains("[ ]"), "{texts:?}");
}

#[test]
fn empty_markdown_renders_no_lines() {
    for markdown in ["", "   "] {
        let (_, texts) = show(markdown);
        assert!(texts.is_empty(), "{markdown:?} -> {texts:?}");
    }
}

#[test]
fn escape_sequences_vanish_without_styling() {
    let (cell, texts) = show("a\u{1b}[31mb\u{7}c");
    assert_eq!(texts, ["abc"]);
    assert_eq!(style_at(&cell, 0, 0), Style::default());
}

#[test]
fn carriage_returns_become_soft_breaks() {
    let (_, texts) = show("a\rb");
    assert_eq!(texts, ["a b"]);
}

#[test]
fn inline_code_has_background_only_on_code() {
    let (cell, texts) = show("a `x = 1` b");
    assert_eq!(texts, ["a x = 1 b"]);
    assert_eq!(
        common::code_bg(&cell.lines[0]),
        vec![
            ("a ".to_owned(), None),
            ("x = 1".to_owned(), Some(europa_core::Color::Indexed(236))),
            (" b".to_owned(), None),
        ]
    );
    for span in &cell.lines[0].spans {
        assert_eq!(span.style.fg, None, "{span:?}");
    }
}

#[test]
fn fenced_code_keeps_breaks_and_fill() {
    let (cell, texts) = show("```py\nif x:\n    y\n\nz\n```");
    assert_eq!(texts, ["if x:", "    y", "", "z"]);
    assert!(!texts.concat().contains("py"), "{texts:?}");
    for line in &cell.lines {
        assert_eq!(
            line.fill,
            Some(europa_core::Color::Indexed(236)),
            "{line:?}"
        );
    }
    let (light, _) = show_with("```py\nif x:\n    y\n\nz\n```", &common::light(80));
    for line in &light.lines {
        assert_eq!(
            line.fill,
            Some(europa_core::Color::Indexed(254)),
            "{line:?}"
        );
    }
    assert_eq!(light.lines.len(), cell.lines.len());
}

#[test]
fn long_code_lines_wrap_with_fill() {
    let (cell, texts) = show_with("```\n012345678901\n```", &common::opts(10));
    assert!(texts.len() > 1, "{texts:?}");
    for line in &cell.lines {
        assert_eq!(line.fill, Some(europa_core::Color::Indexed(236)));
    }
}

#[test]
fn code_in_lists_is_filled_with_indent() {
    let (cell, texts) = show("- a\n\n  ```\n  c\n  ```\n");
    assert!(texts.iter().any(|line| line.contains('c')), "{texts:?}");
    let line = cell
        .lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.text.contains('c')))
        .expect("code line");
    assert_eq!(line.fill, Some(europa_core::Color::Indexed(236)));
    assert!(line.spans[0].text.starts_with(' '));
}

#[test]
fn bullets_and_numbers_render_one_line_each() {
    let (_, texts) = show("- a\n- b\n- c");
    assert_eq!(texts, ["\u{2023} a", "\u{2023} b", "\u{2023} c"]);
    let (_, texts) = show("3. a\n4. b");
    assert_eq!(texts, ["3. a", "4. b"]);
    let (_, texts) = show("- a\n\n- b");
    assert_eq!(texts, ["\u{2023} a", "", "\u{2023} b"]);
}

#[test]
fn nested_items_indent_with_depth_marks() {
    let (_, texts) = show("- a\n  - b\n    - c\n      - d");
    assert_eq!(
        texts,
        [
            "\u{2023} a",
            "  \u{25aa} b",
            "    \u{25b8} c",
            "      \u{25b8} d",
        ]
    );
    let (_, texts) = show("- a\n  - b");
    assert_eq!(texts, ["\u{2023} a", "  \u{25aa} b"]);
}

#[test]
fn long_items_wrap_with_indent() {
    let (cell, texts) = show_with(&format!("- {}", "x".repeat(30)), &common::opts(20));
    assert!(texts.len() > 1, "{texts:?}");
    for line in texts.iter().skip(1) {
        assert!(line.starts_with("  "), "{line:?}");
    }
    let _ = cell;
}

#[test]
fn quotes_keep_marks_on_every_line() {
    let (cell, texts) = show("> q");
    assert_eq!(texts, ["\u{254f} q"]);
    assert!(style_at(&cell, 0, 0).dim);
    let (_, texts) = show_with("> a\n>\n> b", &common::opts(80));
    assert_eq!(texts, ["\u{254f} a", "\u{254f}", "\u{254f} b"]);
    let long = format!("> {}", "y".repeat(30));
    let (_, texts) = show_with(&long, &common::opts(10));
    assert!(texts.len() > 1);
    for line in &texts {
        assert!(line.starts_with("\u{254f} "), "{line:?}");
    }
}

#[test]
fn rules_fill_the_width() {
    let (cell, texts) = show_with("---", &common::opts(20));
    assert_eq!(texts, ["\u{254c}".repeat(20)]);
    assert!(style_at(&cell, 0, 0).dim);
    let (_, texts) = show_with("1. ---", &common::opts(20));
    assert_eq!(texts, [format!("1. {}", "\u{254c}".repeat(17))]);
}

#[test]
fn blanks_around_code_have_no_fill() {
    let (cell, texts) = show("p\n\n```\nc\n```\n\nq");
    assert_eq!(texts, ["p", "", "c", "", "q"]);
    assert_eq!(cell.lines[0].fill, None);
    assert_eq!(cell.lines[1].fill, None);
    assert_eq!(cell.lines[2].fill, Some(europa_core::Color::Indexed(236)));
    assert_eq!(cell.lines[3].fill, None);
    assert_eq!(cell.lines[4].fill, None);
}
