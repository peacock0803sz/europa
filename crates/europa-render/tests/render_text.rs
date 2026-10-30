mod common;

use common::{
    cell_outputs, light, notebook, opts, outputs, plain, stderr, stdout, stream, styled, texts,
};
use europa_core::{Color, JsonMap, Output, RenderedCell, Style};
use europa_render::render_outputs;

const DARK_FILL: Color = Color::Indexed(52);
const LIGHT_FILL: Color = Color::Indexed(224);

/// Display lines of `outputs` in an 80-column area.
fn lines(outputs: &[Output]) -> Vec<String> {
    texts(&render_outputs(outputs, &opts(80)))
}

#[test]
fn no_outputs_render_to_an_empty_cell() {
    assert_eq!(render_outputs(&[], &opts(80)), RenderedCell::default());
}

#[test]
fn rendering_twice_gives_the_same_cell_without_images() {
    let outputs = [stdout("a\n"), stderr("b\n"), plain("c")];
    let first = render_outputs(&outputs, &opts(80));
    assert!(first.images.is_empty());
    assert_eq!(render_outputs(&outputs, &opts(80)), first);
}

#[test]
fn rendering_does_not_change_what_is_saved() {
    let notebook = notebook(
        r#"[
            {"output_type":"stream","name":"stdout","text":"10%\r20%\u001b[31m!\u001b[0m\n"},
            {"output_type":"execute_result","data":{"text/plain":["a\n","b"]},"metadata":{},"execution_count":1}
        ]"#,
    );
    let before = europa_core::serialize(&notebook);
    let cell = render_outputs(cell_outputs(&notebook), &opts(80));
    assert!(!cell.lines.is_empty());
    assert_eq!(europa_core::serialize(&notebook), before);
}

#[test]
fn outputs_follow_the_list_without_blank_lines_between() {
    assert_eq!(lines(&[stdout("a"), stdout("b\n")]), ["ab"]);
    assert_eq!(
        lines(&[stdout("a"), stderr("b"), stdout("c")]),
        ["a", "b", "c"]
    );
    assert_eq!(
        lines(&[stdout("x\n"), plain("y"), stdout("z\n")]),
        ["x", "y", "z"]
    );
}

#[test]
fn only_the_lines_a_body_really_has_are_made() {
    assert_eq!(lines(&[stdout("")]), Vec::<String>::new());
    assert_eq!(lines(&[stdout("a\n")]), ["a"]);
    assert_eq!(lines(&[stdout("\n")]), [""]);
    assert_eq!(lines(&[stdout("a\n\n")]), ["a", ""]);
    assert_eq!(lines(&[stdout("\u{1b}[0m")]), Vec::<String>::new());
    assert_eq!(lines(&[stdout("a\u{1b}[0m")]), ["a"]);
}

#[test]
fn stderr_lines_are_filled_to_the_right_edge_and_stdout_lines_are_not() {
    let outputs = [stdout("o\n"), stderr("e\n")];
    let dark = render_outputs(&outputs, &opts(80));
    assert_eq!(texts(&dark), ["o", "e"]);
    assert_eq!(dark.lines[0].fill, None);
    assert_eq!(dark.lines[1].fill, Some(DARK_FILL));

    let bright = render_outputs(&outputs, &light(80));
    assert_eq!(bright.lines[0].fill, None);
    assert_eq!(bright.lines[1].fill, Some(LIGHT_FILL));

    let custom = render_outputs(&[stream("custom", "x\n")], &opts(80));
    assert_eq!(custom.lines[0].fill, None);
}

#[test]
fn empty_stderr_lines_are_filled_too() {
    let cell = render_outputs(&[stderr("a\n\nb\n")], &opts(80));
    assert_eq!(texts(&cell), ["a", "", "b"]);
    assert!(cell.lines.iter().all(|line| line.fill == Some(DARK_FILL)));
}

#[test]
fn a_light_background_changes_only_the_fill() {
    let outputs = [stdout("o\n"), stderr("\u{1b}[1me\u{1b}[0m\n")];
    let mut dark = render_outputs(&outputs, &opts(80));
    let mut bright = render_outputs(&outputs, &light(80));
    assert_ne!(dark, bright);
    for line in dark.lines.iter_mut().chain(bright.lines.iter_mut()) {
        line.fill = None;
    }
    assert_eq!(dark, bright);
}

#[test]
fn a_background_the_text_sets_wins_over_the_fill_until_it_is_reset() {
    let cell = render_outputs(
        &[stderr("a\u{1b}[44mb\u{1b}[0mc\u{1b}[44md\u{1b}[49me")],
        &opts(80),
    );
    let blue = Style {
        bg: Some(Color::Indexed(4)),
        ..Style::default()
    };
    assert_eq!(
        styled(&cell.lines[0]),
        [
            ("a".to_owned(), Style::default()),
            ("b".to_owned(), blue),
            ("c".to_owned(), Style::default()),
            ("d".to_owned(), blue),
            ("e".to_owned(), Style::default()),
        ]
    );
    assert_eq!(cell.lines[0].fill, Some(DARK_FILL));
}

#[test]
fn the_plain_text_of_a_result_is_shown_instead_of_its_other_forms() {
    let data = JsonMap::from([
        ("text/plain".to_owned(), "42".into()),
        ("text/html".to_owned(), "<b>42</b>".into()),
    ]);
    let result = Output::new_execute_result(data, JsonMap::new(), Some(1));
    assert_eq!(lines(&[result]), ["42"]);
}

#[test]
fn plain_text_kept_as_an_array_is_joined() {
    let display = outputs(
        r#"[{"output_type":"display_data","data":{"text/plain":["a\n","b"]},"metadata":{}}]"#,
    );
    assert_eq!(lines(&display), ["a", "b"]);
}
