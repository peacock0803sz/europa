mod common;

use common::{
    cell_outputs, error, light, notebook, opts, outputs, plain, stderr, stdout, stream, styled,
    texts,
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
fn the_lines_a_stderr_line_wraps_into_are_all_filled() {
    let cell = render_outputs(&[stderr("abcdefghijklmnopqrstuvwxyz0123")], &opts(20));
    assert_eq!(texts(&cell), ["abcdefghijklmnopqrst", "uvwxyz0123"]);
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

#[test]
fn a_traceback_is_joined_with_newlines_and_every_line_is_filled() {
    let cell = render_outputs(
        &[error(
            "ValueError",
            "bad",
            &["head", "frame\n", "ValueError: bad"],
        )],
        &opts(80),
    );
    assert_eq!(texts(&cell), ["head", "frame", "", "ValueError: bad"]);
    assert!(cell.lines.iter().all(|line| line.fill == Some(DARK_FILL)));
}

#[test]
fn an_empty_traceback_is_replaced_by_the_name_and_value() {
    for traceback in [&[][..], &["", ""][..]] {
        let cell = render_outputs(&[error("ValueError", "bad", traceback)], &opts(80));
        assert_eq!(texts(&cell), ["ValueError: bad"], "{traceback:?}");
        assert_eq!(cell.lines[0].fill, Some(DARK_FILL), "{traceback:?}");
    }
}

#[test]
fn a_value_with_newlines_makes_several_lines() {
    assert_eq!(lines(&[error("E", "a\nb", &[])]), ["E: a", "b"]);
}

#[test]
fn every_line_of_a_value_with_newlines_is_filled() {
    let cell = render_outputs(&[error("E", "a\nb", &[])], &opts(80));
    assert_eq!(texts(&cell), ["E: a", "b"]);
    assert!(cell.lines.iter().all(|line| line.fill == Some(DARK_FILL)));
}

#[test]
fn a_color_continues_from_one_traceback_entry_to_the_next() {
    let cell = render_outputs(
        &[error("E", "v", &["\u{1b}[31mred", "next\u{1b}[0m"])],
        &opts(80),
    );
    assert_eq!(texts(&cell), ["red", "next"]);
    let red = Style {
        fg: Some(Color::Indexed(1)),
        ..Style::default()
    };
    assert_eq!(styled(&cell.lines[1]), [("next".to_owned(), red)]);
}

#[test]
fn a_color_set_in_the_middle_of_an_entry_runs_until_it_is_reset() {
    let cell = render_outputs(
        &[error(
            "E",
            "v",
            &["\u{1b}[31mone\ntwo", "three\u{1b}[0m\nfour", "five"],
        )],
        &opts(80),
    );
    assert_eq!(texts(&cell), ["one", "two", "three", "four", "five"]);
    let colors: Vec<_> = cell
        .lines
        .iter()
        .map(|line| line.spans[0].style.fg)
        .collect();
    let red = Some(Color::Indexed(1));
    assert_eq!(colors, [red, red, red, None, None]);
    assert!(cell.lines.iter().all(|line| line.fill == Some(DARK_FILL)));
}

#[test]
fn error_lines_are_filled_for_a_light_background_too() {
    let cell = render_outputs(&[error("E", "v", &["t"])], &light(80));
    assert_eq!(cell.lines[0].fill, Some(LIGHT_FILL));
}

#[test]
fn a_style_does_not_carry_from_a_stream_into_an_error() {
    let cell = render_outputs(&[stdout("\u{1b}[31mx"), error("E", "v", &["t"])], &opts(80));
    assert_eq!(texts(&cell), ["x", "t"]);
    assert_eq!(styled(&cell.lines[1]), [("t".to_owned(), Style::default())]);
}
