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
fn links_show_targets_after_words() {
    let (cell, texts) = show("[docs](https://example.com)");
    assert_eq!(texts, ["docs (https://example.com)"]);
    let spans = common::styled(&cell.lines[0]);
    assert_eq!(spans.len(), 3);
    assert!(spans[0].1.underline);
    assert!(!spans[0].1.dim);
    assert_eq!(spans[1], (" ".to_owned(), europa_core::Style::default()));
    assert!(spans[2].1.dim);
    let (cell, texts) = show("<https://example.com>");
    assert_eq!(texts, ["https://example.com"]);
    assert!(style_at(&cell, 0, 0).underline);
    let (_, texts) = show("[https://example.com](https://example.com)");
    assert_eq!(texts, ["https://example.com"]);
    let (cell, _) = show("[**docs**](https://example.com)");
    let style = style_at(&cell, 0, 0);
    assert!(style.bold && style.underline);
    let (_, inline) = show("[docs](https://example.com)");
    let (_, reference) = show("[docs][1]\n\n[1]: https://example.com");
    assert_eq!(inline, reference);
}

#[test]
fn long_link_targets_wrap() {
    let (cell, texts) = show_with("[x](https://example.com/very/long/path)", &common::opts(20));
    assert!(texts
        .concat()
        .contains("https://example.com/very/long/path"));
    let _ = cell;
}

#[test]
fn images_become_notices() {
    let (cell, texts) = show("![plot](a.png)");
    assert_eq!(texts, ["[image: plot]"]);
    assert!(style_at(&cell, 0, 0).dim);
    assert!(!texts.concat().contains("a.png"));
}

#[test]
fn tables_align_columns() {
    let (cell, texts) = show("| name | value |\n|---|--:|\n| a | 1 |\n| bb | 22 |");
    assert_eq!(
        texts,
        ["name \u{254e} value", "\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}\u{254c}", "a    \u{254e}     1", "bb   \u{254e}    22"]
    );
    assert!(style_at(&cell, 0, 0).bold);
    assert!(style_at(&cell, 1, 0).dim);
}

#[test]
fn centered_remainder_goes_right() {
    let (_, texts) = show("| xxxx | y |\n|:----:|---|\n| a | b |");
    assert!(texts.iter().any(|line| line.contains(" a  ")), "{texts:?}");
}

#[test]
fn wide_characters_align_separators() {
    let (_, texts) = show("| \u{540d}\u{524d} | x |\n|---|---|\n| a | y |");
    let mut columns = Vec::new();
    for line in &texts {
        if line.contains('\u{254e}') {
            let before = line.split('\u{254e}').next().expect("cell");
            columns.push(europa_core::Style::default());
            let _ = before;
        }
    }
    assert!(!texts.is_empty());
    let positions: Vec<usize> = texts
        .iter()
        .filter(|line| line.contains('\u{254e}'))
        .map(|line| {
            let before = line.split('\u{254e}').next().expect("cell");
            unicode_width::UnicodeWidthStr::width(before)
        })
        .collect();
    assert!(!positions.is_empty());
    assert!(
        positions.windows(2).all(|pair| pair[0] == pair[1]),
        "{texts:?}"
    );
}

#[test]
fn narrow_tables_wrap_without_separators() {
    let markdown = "| name | value |\n|---|--:|\n| a | 1 |\n| bb | 22 |";
    let (cell, texts) = show_with(markdown, &common::opts(10));
    assert!(!texts.concat().contains('\u{254c}'), "{texts:?}");
    for expected in ["name", "value", "a", "1", "bb", "22"] {
        assert!(
            texts.concat().contains(expected),
            "{texts:?} missing {expected}"
        );
    }
    let _ = cell;
}

#[test]
fn missing_cells_are_empty_and_extra_cells_drop() {
    let (_, texts) = show("| a | b |\n|---|---|\n| c |\n| d | e | f |");
    assert!(texts.iter().any(|line| line.contains("c")), "{texts:?}");
    assert!(!texts.concat().contains('f'), "{texts:?}");
}

#[test]
fn code_in_cells_has_no_padded_background() {
    let (cell, _) = show("| a | b |\n|---|---|\n| `c` | d |");
    let line = cell
        .lines
        .iter()
        .find(|line| line.spans.iter().any(|span| span.text == "c"))
        .expect("code cell");
    for (text, bg) in common::code_bg(line) {
        if text.contains('c') && !text.contains(' ') {
            assert_eq!(bg, Some(europa_core::Color::Indexed(236)), "{text:?}");
        } else if text.trim().is_empty() {
            assert_eq!(bg, None, "{text:?}");
        }
    }
}

#[test]
fn raw_html_follows_html_rules() {
    let (_, texts) = show("<div><p>x</p></div>");
    assert_eq!(texts, ["x"]);
    let (cell, texts) = show("a <b>b</b> c");
    assert_eq!(texts, ["a b c"]);
    let body: String = cell.lines[0]
        .spans
        .iter()
        .map(|span| span.text.as_str())
        .collect();
    assert!(style_at(&cell, 0, body.find('b').expect("b")).bold);
    let (_, texts) = show("a<br>b");
    assert_eq!(texts, ["a", "b"]);
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
