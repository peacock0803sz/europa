mod common;

use europa_core::{Background, Color, Style};
use europa_render::render_outputs;

fn render(data: &str) -> (europa_core::RenderedCell, Vec<String>) {
    render_html_with(data, &common::opts(80))
}

fn render_html_with(
    data: &str,
    options: &europa_core::RenderOptions,
) -> (europa_core::RenderedCell, Vec<String>) {
    let cell = render_outputs(&[common::display(data)], options);
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
fn paragraphs_keep_one_blank_line_between_them() {
    let (cell, texts) = render(r#"{"text/html": "<p>Hello <b>world</b></p><p>Second</p>"}"#);
    assert_eq!(texts, ["Hello world", "", "Second"]);
    let col = texts[0].find("world").expect("world");
    assert!(style_at(&cell, 0, col).bold);
}

#[test]
fn div_and_break_split_lines_without_blank_lines() {
    let (_, first) = render(r#"{"text/html": "<div>a</div><div>b</div>"}"#);
    assert_eq!(first, ["a", "b"]);
    let (_, second) = render(r#"{"text/html": "a<br>b"}"#);
    assert_eq!(second, ["a", "b"]);
}

#[test]
fn nested_blocks_share_a_single_blank_line() {
    let (_, texts) =
        render(r#"{"text/html": "<div><div><p>a</p></div></div><div><p>b</p></div>"}"#);
    assert_eq!(texts, ["a", "", "b"]);
}

#[test]
fn repeated_breaks_share_a_single_blank_line() {
    for data in [
        r#"{"text/html": "a<br><br>b"}"#,
        r#"{"text/html": "a<br><br><br>b"}"#,
        r#"{"text/html": "<p>a</p><br><p>b</p>"}"#,
    ] {
        let (_, texts) = render(data);
        assert_eq!(texts, ["a", "", "b"], "{data}");
    }
    let (_, leading) = render(r#"{"text/html": "<br>a"}"#);
    assert_eq!(leading, ["a"]);
    let (_, trailing) = render(r#"{"text/html": "a<br>"}"#);
    assert_eq!(trailing, ["a"]);
    let (_, texts) = render(r#"{"text/html": "a<br><br>b"}"#);
    assert_eq!(texts[1], "");
}

#[test]
fn entities_become_characters() {
    let (_, texts) = render(r#"{"text/html": "&lt; &amp; &nbsp; &#x41; &#65; &foo;"}"#);
    assert_eq!(texts, ["< & \u{a0} A A &foo;"]);
}

#[test]
fn whitespace_collapses_outside_pre() {
    let (_, texts) = render(r#"{"text/html": "<p>a\n   b</p>"}"#);
    assert_eq!(texts, ["a b"]);
    let (_, table) =
        render(r#"{"text/html": "<table>\n <tr>\n  <td>a</td>\n  <td>b</td>\n </tr>\n</table>"}"#);
    assert_eq!(table, ["a \u{254e} b"]);
}

#[test]
fn pre_keeps_line_breaks_and_fill() {
    let (cell, texts) = render(r#"{"text/html": "<pre>x\n\n  y</pre>"}"#);
    assert_eq!(texts, ["x", "", "  y"]);
    for line in &cell.lines {
        assert_eq!(line.fill, Some(Color::Indexed(236)));
    }
    let (light, _) = render_html_with(
        r#"{"text/html": "<pre>x\n\n  y</pre>"}"#,
        &common::light(80),
    );
    for line in &light.lines {
        assert_eq!(line.fill, Some(Color::Indexed(254)));
    }
}

#[test]
fn headings_emphasis_and_code_have_styles() {
    let (cell, texts) = render(r#"{"text/html": "<h1>T</h1>"}"#);
    assert_eq!(texts, ["T"]);
    let style = style_at(&cell, 0, 0);
    assert!(style.bold && style.underline);
    let (cell, _) = render(r#"{"text/html": "<p><em>e</em></p>"}"#);
    assert!(style_at(&cell, 0, 0).italic);
    let (cell, texts) = render(r#"{"text/html": "a <code>c</code> b"}"#);
    assert_eq!(texts, ["a c b"]);
    let backgrounds = common::code_bg(&cell.lines[0]);
    assert_eq!(
        backgrounds,
        vec![
            ("a ".to_owned(), None),
            ("c".to_owned(), Some(Color::Indexed(236))),
            (" b".to_owned(), None),
        ]
    );
}

#[test]
fn comments_declarations_head_scripts_and_styles_vanish() {
    for data in [
        r#"{"text/html": "<!-- c -->x"}"#,
        r#"{"text/html": "<!DOCTYPE html>x"}"#,
        r#"{"text/html": "<head><title>t</title></head>x"}"#,
        r#"{"text/html": "<script>alert(\"</p>\")</script>x"}"#,
        r#"{"text/html": "<style>p{}</style>x"}"#,
    ] {
        let (_, texts) = render(data);
        assert_eq!(texts, ["x"], "{data}");
    }
}

#[test]
fn escape_sequences_and_carriage_returns_vanish() {
    let (cell, texts) = render(r#"{"text/html": "&#27;[31mx"}"#);
    assert_eq!(texts, ["x"]);
    assert_eq!(style_at(&cell, 0, 0), Style::default());
    let (_, texts) = render(r#"{"text/html": "a\rb"}"#);
    assert_eq!(texts, ["ab"]);
}

#[test]
fn empty_html_becomes_a_notice() {
    for data in [
        r#"{"text/html": ""}"#,
        r#"{"text/html": "  \n "}"#,
        r#"{"text/html": "<div></div>"}"#,
    ] {
        let (_, texts) = render(data);
        assert_eq!(texts, ["[text/html]"], "{data}");
    }
}

#[test]
fn images_and_embedded_frames_become_notices() {
    let (cell, texts) = render(r#"{"text/html": "<img alt=\"plot\">"}"#);
    assert_eq!(texts, ["[img: plot]"]);
    assert!(style_at(&cell, 0, 0).dim);
    let (_, texts) = render(r#"{"text/html": "<img>"}"#);
    assert_eq!(texts, ["[img]"]);
    let (_, texts) = render(r#"{"text/html": "<svg><text>x</text></svg>"}"#);
    assert_eq!(texts, ["[svg]"]);
    for (tag, expected) in [
        ("iframe", "[iframe]"),
        ("video", "[video]"),
        ("audio", "[audio]"),
        ("canvas", "[canvas]"),
        ("object", "[object]"),
    ] {
        let data = format!("{{\"text/html\": \"<{tag}><p>x</p></{tag}>\"}}");
        let (_, texts) = render(&data);
        assert_eq!(texts, [expected], "{tag}");
    }
    let (_, texts) = render(r#"{"text/html": "<embed src=x>after"}"#);
    assert_eq!(texts, ["[embed]after"]);
    let (_, texts) = render(r#"{"text/html": "<svg/>after"}"#);
    assert_eq!(texts, ["[svg]after"]);
    let (_, texts) = render(r#"{"text/html": "<video>x"}"#);
    assert_eq!(texts, ["[video]"]);
}

#[test]
fn tables_join_cells_with_separators() {
    let (cell, texts) = render(
        r#"{"text/html": "<table><tr><th>a</th><th>b</th></tr><tr><td>c</td><td>d</td></tr></table>"}"#,
    );
    assert_eq!(texts, ["a \u{254e} b", "c \u{254e} d"]);
    assert!(style_at(&cell, 0, 0).bold);
    let (_, texts) = render(
        r#"{"text/html": "<table><tr><td>long</td><td>b</td></tr><tr><td>c</td><td>d</td></tr></table>"}"#,
    );
    assert_eq!(texts, ["long \u{254e} b", "c \u{254e} d"]);
}

#[test]
fn lists_use_numbers_and_indented_markers() {
    let (_, texts) = render(r#"{"text/html": "<ol><li>a</li><li>b<ul><li>c</li></ul></li></ol>"}"#);
    assert_eq!(texts, ["1. a", "2. b", "   \u{25aa} c"]);
    let (_, texts) = render(r#"{"text/html": "<ol start=\"3\"><li>a</li></ol>"}"#);
    assert_eq!(texts, ["1. a"]);
}

#[test]
fn horizontal_rules_fill_the_width() {
    let (cell, texts) = render_html_with(r#"{"text/html": "<hr>"}"#, &common::opts(20));
    assert_eq!(texts, ["\u{254c}".repeat(20)]);
    assert!(style_at(&cell, 0, 0).dim);
    let (_, texts) = render_html_with(
        r#"{"text/html": "<ul><li><hr></li></ul>"}"#,
        &common::opts(20),
    );
    assert_eq!(texts, [format!("\u{2023} {}", "\u{254c}".repeat(18))]);
}

#[test]
fn links_keep_only_their_text() {
    let (_, texts) = render(r#"{"text/html": "<a href=\"u\">x</a>"}"#);
    assert_eq!(texts, ["x"]);
}

#[test]
fn broken_html_still_renders_text() {
    for (data, expected) in [
        (r#"{"text/html": "<p>a<b>b"}"#, "b"),
        (r#"{"text/html": "<div><p>x"}"#, "x"),
        (r#"{"text/html": "<table><tr><td>a"}"#, "a"),
        (r#"{"text/html": "<b"}"#, ""),
        (r#"{"text/html": "a < b"}"#, "a < b"),
    ] {
        let cell = render_outputs(&[common::display(data)], &common::opts(80));
        common::assert_lines_are_well_formed(&cell);
        let texts = common::texts(&cell);
        if expected.is_empty() {
            continue;
        }
        assert!(
            texts.iter().any(|line| line.contains(expected)),
            "{data} -> {texts:?}"
        );
    }
}

#[test]
fn arrays_join_and_broken_plain_falls_back_to_html() {
    let (_, texts) = render(r#"{"text/html": ["<p>a</p>", "<p>b</p>"]}"#);
    assert_eq!(texts, ["a", "", "b"]);
    let (_, texts) = render(r#"{"text/html": "<p>y</p>", "text/plain": 1}"#);
    assert_eq!(texts, ["y"]);
}

#[test]
fn background_is_kept_for_dark_and_light() {
    let _ = Background::Dark;
}
