mod common;

use common::{line_width, opts, stderr, stdout, styled, texts};
use europa_core::{Color, Output, Style, StyledLine};
use europa_render::render_outputs;
use unicode_segmentation::UnicodeSegmentation;

/// Display lines of the stdout `body` in an area `width` columns wide.
fn wrapped(body: &str, width: u16) -> Vec<String> {
    texts(&render_outputs(&[stdout(body)], &opts(width)))
}

fn is_one_cluster(line: &StyledLine) -> bool {
    let text: String = line.spans.iter().map(|span| span.text.as_str()).collect();
    text.graphemes(true).count() == 1
}

#[test]
fn a_long_line_is_cut_where_the_width_is_reached() {
    assert_eq!(
        wrapped("abcdefghijklmnopqrstuvwxyz0123", 20),
        ["abcdefghijklmnopqrst", "uvwxyz0123"]
    );
}

#[test]
fn a_word_is_cut_in_the_middle_and_spaces_stay_where_they_fall() {
    assert_eq!(wrapped("aaaa bbbbbbbb", 10), ["aaaa bbbbb", "bbb"]);
    assert_eq!(wrapped("aaaa bbbbbbbb", 5), ["aaaa ", "bbbbb", "bbb"]);
}

#[test]
fn a_wide_character_moves_to_the_next_line_instead_of_splitting() {
    assert_eq!(wrapped("あいう", 5), ["あい", "う"]);
    assert_eq!(wrapped("a\u{3042}", 2), ["a", "\u{3042}"]);
    assert_eq!(wrapped("あい", 1), ["あ", "い"]);
}

#[test]
fn combining_marks_and_joined_emoji_are_not_split() {
    assert_eq!(
        wrapped("e\u{301}e\u{301}e\u{301}", 2),
        ["e\u{301}e\u{301}", "e\u{301}"]
    );
    let family = "\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}";
    let lines = wrapped(&format!("a{family}b"), 2);
    assert!(lines.iter().any(|line| line.contains(family)), "{lines:?}");
}

#[test]
fn a_cluster_split_between_styles_takes_the_style_of_its_first_character() {
    let cell = render_outputs(&[stdout("e\u{1b}[31m\u{301}x")], &opts(80));
    assert_eq!(
        styled(&cell.lines[0]),
        [
            ("e\u{301}".to_owned(), Style::default()),
            (
                "x".to_owned(),
                Style {
                    fg: Some(Color::Indexed(1)),
                    ..Style::default()
                }
            ),
        ]
    );
}

#[test]
fn a_tab_runs_to_the_next_multiple_of_eight_counted_from_the_start_of_the_line() {
    assert_eq!(wrapped("a\tb", 80), ["a       b"]);
    assert_eq!(wrapped("abcdefghij\tX", 10), ["abcdefghij", "      X"]);
}

#[test]
fn the_spaces_of_a_tab_have_the_style_of_the_tab() {
    let cell = render_outputs(&[stdout("\u{1b}[31ma\tb")], &opts(80));
    assert_eq!(
        styled(&cell.lines[0]),
        [(
            "a       b".to_owned(),
            Style {
                fg: Some(Color::Indexed(1)),
                ..Style::default()
            }
        )]
    );
}

#[test]
fn a_line_that_fills_the_width_exactly_adds_no_empty_line() {
    assert_eq!(wrapped("abcde", 5), ["abcde"]);
    assert_eq!(wrapped("abcde\n", 5), ["abcde"]);
}

#[test]
fn wrapped_lines_keep_their_style() {
    let cell = render_outputs(&[stdout("\u{1b}[31mabcdef")], &opts(3));
    assert_eq!(texts(&cell), ["abc", "def"]);
    for line in &cell.lines {
        assert_eq!(line.spans.len(), 1);
        assert_eq!(line.spans[0].style.fg, Some(Color::Indexed(1)));
    }
}

#[test]
fn no_line_is_wider_than_the_area_and_no_character_is_lost_or_added() {
    let outputs: [Output; 12] = [
        stdout("abcdefghijklmnopqrstuvwxyz0123"),
        stdout("aaaa bbbbbbbb"),
        stdout("\u{3042}\u{3044}\u{3046}"),
        stdout("a\u{3042}"),
        stdout("\u{3042}\u{3044}"),
        stdout("e\u{301}e\u{301}e\u{301}"),
        stdout("a\u{1f468}\u{200d}\u{1f469}\u{200d}\u{1f467}b"),
        stdout("a\tb"),
        stdout("abcdefghij\tX"),
        stdout("abcde"),
        stdout("abcde\n"),
        stderr("\u{5168}\u{89d2}\u{3068}half\u{3068}\t\u{30bf}\u{30d6}"),
    ];
    for output in &outputs {
        let one = std::slice::from_ref(output);
        let whole = texts(&render_outputs(one, &opts(u16::MAX))).concat();
        for width in 1..=12 {
            let cell = render_outputs(one, &opts(width));
            for line in &cell.lines {
                assert!(
                    line_width(line) <= usize::from(width) || is_one_cluster(line),
                    "{output:?} at {width}: {line:?}"
                );
            }
            assert_eq!(texts(&cell).concat(), whole, "{output:?} at {width}");
        }
    }
}
