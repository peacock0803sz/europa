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
