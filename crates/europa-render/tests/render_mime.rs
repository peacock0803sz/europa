mod common;

use europa_render::render_outputs;

fn show(data: &str) -> Vec<String> {
    let cell = render_outputs(&[common::display(data)], &common::opts(80));
    common::assert_lines_are_well_formed(&cell);
    common::texts(&cell)
}

#[test]
fn json_is_shown_instead_of_the_default_repr() {
    let texts = show(
        r#"{"application/json": {"a": 1}, "text/plain": "<IPython.core.display.JSON object>"}"#,
    );
    assert!(texts.iter().any(|line| line.contains("\"a\"")), "{texts:?}");
    assert!(!texts.concat().contains("IPython"), "{texts:?}");
}

#[test]
fn a_table_in_plain_text_wins_over_html() {
    let texts = show(
        r#"{"text/html": "<table><tr><td>1</td></tr></table>", "text/plain": "   a\n0  1\n1  2"}"#,
    );
    assert!(texts.iter().any(|line| line.contains("0  1")), "{texts:?}");
}

#[test]
fn a_default_repr_gives_way_to_html() {
    let texts =
        show(r#"{"text/html": "<p>x</p>", "text/plain": "<IPython.core.display.HTML object>"}"#);
    assert_eq!(texts, ["x"]);
}

#[test]
fn surrounding_blanks_do_not_hide_a_default_repr() {
    let texts = show(r#"{"text/html": "<p>x</p>", "text/plain": "  <X object>\n"}"#);
    assert_eq!(texts, ["x"]);
}

#[test]
fn other_reprs_stay_with_plain_text() {
    let texts = show(r#"{"text/html": "<p>x</p>", "text/plain": "<__main__.Foo at 0x7f00>"}"#);
    assert!(texts.concat().contains("Foo"), "{texts:?}");
    let two_lines = show(r#"{"text/html": "<p>x</p>", "text/plain": "<a object>\n<a object>"}"#);
    assert!(
        two_lines.iter().any(|line| line.contains("<a object>"),),
        "{two_lines:?}"
    );
}

#[test]
fn a_default_repr_without_usable_html_stays() {
    let texts = show(r#"{"text/plain": "<IPython.core.display.HTML object>"}"#);
    assert!(texts.concat().contains("HTML object"), "{texts:?}");
    let broken = show(r#"{"text/html": 1, "text/plain": "<IPython.core.display.HTML object>"}"#);
    assert!(broken.concat().contains("HTML object"), "{broken:?}");
}

#[test]
fn an_image_and_plain_text_show_the_plain_text() {
    let texts = show(r#"{"image/png": "iVBORw0KGgo=", "text/plain": "plain text"}"#);
    assert_eq!(texts, ["plain text"]);
}

#[test]
fn broken_markdown_falls_back_to_plain_text() {
    let texts = show(r#"{"text/markdown": 42, "text/plain": "fallback"}"#);
    assert_eq!(texts, ["fallback"]);
}

#[test]
fn unknown_json_kinds_fall_back_to_plain_text_or_a_notice() {
    let texts = show(r#"{"application/vnd.vegalite.v5+json": {"a": 1}, "text/plain": "fallback"}"#);
    assert_eq!(texts, ["fallback"]);
    let notice = show(r#"{"application/vnd.vegalite.v5+json": {"a": 1}}"#);
    assert_eq!(notice, ["[application/vnd.vegalite.v5+json]"]);
}

#[test]
fn html_arrays_join_their_parts() {
    let texts = show(r#"{"text/html": ["<p>a</p>", "<p>b</p>"]}"#);
    assert_eq!(texts, ["a", "", "b"]);
}

#[test]
fn streams_errors_and_unknown_outputs_stay() {
    let cell = render_outputs(&[common::stdout("ok\n")], &common::opts(80));
    assert_eq!(common::texts(&cell), ["ok"]);
    let cell = render_outputs(&[common::error("E", "bad", &["line"])], &common::opts(80));
    assert!(common::texts(&cell).concat().contains("line"));
}
