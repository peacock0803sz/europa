mod common;

use common::{opts, plain, stderr, stdout, styled, texts};
use europa_core::{Color, Output, RenderedCell, Style};
use europa_render::render_outputs;

/// Render in an 80-column area, and check that nothing in the lines could
/// change the state of a terminal.
fn render(outputs: &[Output]) -> RenderedCell {
    let cell = render_outputs(outputs, &opts(80));
    let controls = cell
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .any(|span| span.text.chars().any(char::is_control));
    assert!(!controls, "a control character is left in {cell:?}");
    cell
}

/// Display lines of the stdout `body`.
fn lines(body: &str) -> Vec<String> {
    texts(&render(&[stdout(body)]))
}

/// The spans of the first display line of the stdout `body`.
fn spans(body: &str) -> Vec<(String, Style)> {
    styled(&render(&[stdout(body)]).lines[0])
}

fn fg(index: u8) -> Style {
    Style {
        fg: Some(Color::Indexed(index)),
        ..Style::default()
    }
}

fn bg(index: u8) -> Style {
    Style {
        bg: Some(Color::Indexed(index)),
        ..Style::default()
    }
}

fn bold() -> Style {
    Style {
        bold: true,
        ..Style::default()
    }
}

#[test]
fn a_color_lasts_until_it_is_reset() {
    assert_eq!(
        spans("\u{1b}[31mred\u{1b}[0m plain"),
        [
            ("red".to_owned(), fg(1)),
            (" plain".to_owned(), Style::default())
        ]
    );
}

#[test]
fn each_number_sets_its_attribute_or_color() {
    let cases = [
        ("1", bold()),
        (
            "2",
            Style {
                dim: true,
                ..Style::default()
            },
        ),
        (
            "3",
            Style {
                italic: true,
                ..Style::default()
            },
        ),
        (
            "4",
            Style {
                underline: true,
                ..Style::default()
            },
        ),
        (
            "7",
            Style {
                reverse: true,
                ..Style::default()
            },
        ),
        (
            "9",
            Style {
                strikethrough: true,
                ..Style::default()
            },
        ),
        ("31", fg(1)),
        ("37", fg(7)),
        ("90", fg(8)),
        ("97", fg(15)),
        ("41", bg(1)),
        ("47", bg(7)),
        ("100", bg(8)),
        ("107", bg(15)),
        ("38;5;196", fg(196)),
        ("48;5;21", bg(21)),
        (
            "38;2;1;2;3",
            Style {
                fg: Some(Color::Rgb(1, 2, 3)),
                ..Style::default()
            },
        ),
        (
            "48;2;4;5;6",
            Style {
                bg: Some(Color::Rgb(4, 5, 6)),
                ..Style::default()
            },
        ),
    ];
    for (code, style) in cases {
        assert_eq!(
            spans(&format!("\u{1b}[{code}mX")),
            [("X".to_owned(), style)],
            "code {code}"
        );
    }
}

#[test]
fn an_off_number_undoes_only_what_it_names() {
    let all = Style {
        fg: Some(Color::Indexed(1)),
        bg: Some(Color::Indexed(1)),
        bold: true,
        dim: true,
        italic: true,
        underline: true,
        reverse: true,
        strikethrough: true,
    };
    let cases = [
        (
            "22",
            Style {
                bold: false,
                dim: false,
                ..all
            },
        ),
        (
            "23",
            Style {
                italic: false,
                ..all
            },
        ),
        (
            "24",
            Style {
                underline: false,
                ..all
            },
        ),
        (
            "27",
            Style {
                reverse: false,
                ..all
            },
        ),
        (
            "29",
            Style {
                strikethrough: false,
                ..all
            },
        ),
        ("39", Style { fg: None, ..all }),
        ("49", Style { bg: None, ..all }),
    ];
    for (code, style) in cases {
        assert_eq!(
            spans(&format!("\u{1b}[1;2;3;4;7;9;31;41m\u{1b}[{code}mX")),
            [("X".to_owned(), style)],
            "code {code}"
        );
    }
}

#[test]
fn zero_and_an_empty_sequence_undo_everything() {
    for reset in ["\u{1b}[0m", "\u{1b}[m"] {
        assert_eq!(
            spans(&format!("\u{1b}[1;2;3;4;7;9;31;41m{reset}X")),
            [("X".to_owned(), Style::default())],
            "{reset:?}"
        );
    }
}

#[test]
fn restoring_the_color_keeps_the_attributes() {
    assert_eq!(
        spans("\u{1b}[1;31mA\u{1b}[39mB"),
        [
            (
                "A".to_owned(),
                Style {
                    fg: Some(Color::Indexed(1)),
                    bold: true,
                    ..Style::default()
                }
            ),
            ("B".to_owned(), bold()),
        ]
    );
}

#[test]
fn a_style_continues_over_newlines_within_one_output() {
    let cell = render(&[stdout("\u{1b}[31ma\nb")]);
    assert_eq!(texts(&cell), ["a", "b"]);
    assert_eq!(styled(&cell.lines[1]), [("b".to_owned(), fg(1))]);
}

#[test]
fn a_style_does_not_carry_into_the_next_output() {
    let cell = render(&[stdout("\u{1b}[31ma"), stderr("b")]);
    assert_eq!(styled(&cell.lines[1]), [("b".to_owned(), Style::default())]);

    let cell = render(&[stdout("\u{1b}[31ma\n"), plain("b")]);
    assert_eq!(styled(&cell.lines[1]), [("b".to_owned(), Style::default())]);
}

#[test]
fn a_style_continues_between_streams_of_one_name() {
    let cell = render(&[stdout("\u{1b}[31ma"), stdout("b")]);
    assert_eq!(texts(&cell), ["ab"]);
    assert_eq!(styled(&cell.lines[0]), [("ab".to_owned(), fg(1))]);
}

#[test]
fn a_color_that_cannot_be_read_changes_nothing() {
    for body in [
        "\u{1b}[58;5;1mX",
        "\u{1b}[38;5mX",
        "\u{1b}[38;2;1;2mX",
        "\u{1b}[38;7;1mX",
        "\u{1b}[38:5:1mX",
    ] {
        assert_eq!(
            spans(body),
            [("X".to_owned(), Style::default())],
            "{body:?}"
        );
    }
}

#[test]
fn a_color_out_of_range_is_skipped_and_the_numbers_after_it_still_apply() {
    for body in ["\u{1b}[38;5;300;1mX", "\u{1b}[38:5:1;1mX"] {
        assert_eq!(spans(body), [("X".to_owned(), bold())], "{body:?}");
    }
}

#[test]
fn a_number_without_a_meaning_here_is_skipped_and_the_numbers_after_it_still_apply() {
    for code in ["5", "6", "8", "21", "53", "60"] {
        assert_eq!(
            spans(&format!("\u{1b}[{code}mX")),
            [("X".to_owned(), Style::default())],
            "code {code}"
        );
    }
    assert_eq!(
        spans("\u{1b}[5;1;53;31mX"),
        [(
            "X".to_owned(),
            Style {
                fg: Some(Color::Indexed(1)),
                bold: true,
                ..Style::default()
            }
        )]
    );
}

#[test]
fn sequences_and_control_characters_leave_only_the_text_around_them() {
    let removed = [
        "\u{1b}[2A",
        "\u{1b}[2J",
        "\u{1b}[?25l",
        "\u{1b}]0;t\u{7}",
        "\u{1b}]0;t\u{1b}\\",
        "\u{1b}P1$r\u{1b}\\",
        "\u{1b}_x\u{1b}\\",
        "\u{1b}(B",
        "\u{1b}7",
        "\u{1b}\\",
        "\u{7}",
        "\u{8}",
        "\u{0}",
        "\u{7f}",
        "\u{9b}",
    ];
    for sequence in removed {
        assert_eq!(lines(&format!("a{sequence}b\n")), ["ab"], "{sequence:?}");
    }
}

#[test]
fn a_sequence_cut_off_by_the_end_of_the_text_is_dropped() {
    assert_eq!(lines("ab\u{1b}[31"), ["ab"]);
}

#[test]
fn a_hyperlink_leaves_only_its_text() {
    assert_eq!(
        lines("\u{1b}]8;;https://example.com\u{1b}\\link\u{1b}]8;;\u{1b}\\\n"),
        ["link"]
    );
}

#[test]
fn a_string_sequence_without_an_end_stops_at_the_line() {
    assert_eq!(lines("a\u{1b}]0;title\nb\n"), ["a", "b"]);
}

#[test]
fn an_eight_bit_control_does_not_start_a_sequence() {
    assert_eq!(lines("a\u{9b}31mb"), ["a31mb"]);
}

#[test]
fn an_escape_before_a_character_beyond_ascii_drops_only_the_escape() {
    assert_eq!(lines("a\u{1b}\u{e9}b"), ["a\u{e9}b"]);
}

#[test]
fn many_unfinished_sequences_in_a_row_are_not_a_problem() {
    assert_eq!(lines(&("\u{1b}]".repeat(100_000) + "\ntext")), ["", "text"]);
    assert_eq!(lines(&("\u{1b}[".repeat(100_000) + "\ntext")), ["", "text"]);
}
