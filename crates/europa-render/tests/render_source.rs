mod common;

use europa_core::{Color, SourceLanguage, Style};
use europa_render::render_source;
use unicode_width::UnicodeWidthStr;

use common::{
    assert_source_lines_are_well_formed, light, limited, line_width, opts, source_lines, styles_of,
};

fn plain(source: &str, width: u16) -> Vec<String> {
    let lines = render_source(source, SourceLanguage::Plain, &opts(width));
    assert_source_lines_are_well_formed(&lines);
    source_lines(&lines)
}

#[test]
fn empty_source_has_no_lines() {
    assert!(plain("", 80).is_empty());
}

#[test]
fn trailing_newline_makes_no_extra_line() {
    assert_eq!(plain("\n\n", 80), ["", ""]);
    assert_eq!(plain("  \n", 80), ["  "]);
    assert_eq!(plain("a\n", 80), ["a"]);
    assert_eq!(plain("a\n\n", 80), ["a", ""]);
}

#[test]
fn crlf_is_a_line_break() {
    assert_eq!(plain("a\r\nb", 80), ["a", "b"]);
}

#[test]
fn tab_expands_to_the_next_stop() {
    assert!(plain("\tx = 1", 80)[0].starts_with("        x"));
}

#[test]
fn long_lines_wrap_at_the_width() {
    assert_eq!(plain("x = \"0123456789abc\"", 10).len(), 2);
    for line in render_source("あいうえおかきくけこ", SourceLanguage::Plain, &opts(10)) {
        assert!(line_width(&line) <= 10);
    }
}

#[test]
fn the_output_limit_does_not_apply() {
    let source = "x\n".repeat(3000);
    let lines = render_source(&source, SourceLanguage::Plain, &limited(80, 1));
    assert_eq!(lines.len(), 3000);
}

#[test]
fn control_characters_become_pictures() {
    let lines = render_source("\u{1b}[31mred", SourceLanguage::Plain, &opts(80));
    assert_source_lines_are_well_formed(&lines);
    assert_eq!(source_lines(&lines), ["␛[31mred"]);
    let control = Style {
        fg: Some(Color::Indexed(1)),
        bold: true,
        ..Style::default()
    };
    assert_eq!(styles_of(&lines, "␛"), [control]);
    assert!(styles_of(&lines, "[31mred")
        .iter()
        .all(|style| *style == Style::default()));

    for (control, picture) in [
        ('\u{0}', "␀"),
        ('\u{7}', "␇"),
        ('\u{7f}', "␡"),
        ('\u{85}', "␦"),
    ] {
        let lines = render_source(&control.to_string(), SourceLanguage::Plain, &opts(80));
        assert_eq!(source_lines(&lines), [picture]);
        assert_eq!(UnicodeWidthStr::width(picture), 1);
    }
}

#[test]
fn characters_survive_apart_from_known_replacements() {
    let source = "a\tb\r\n\u{1b}x\n全角あ\n";
    let expected = "a       b\n␛x\n全角あ";
    let lines = render_source(source, SourceLanguage::Plain, &opts(80));
    assert_eq!(source_lines(&lines).join("\n"), expected);
}

#[test]
fn background_and_repetition_do_not_change_the_result() {
    let source = "x = 1\n\u{1b}y";
    let dark = render_source(source, SourceLanguage::Plain, &opts(80));
    assert_eq!(
        dark,
        render_source(source, SourceLanguage::Plain, &light(80))
    );
    assert_eq!(
        dark,
        render_source(source, SourceLanguage::Plain, &opts(80))
    );
}
