mod common;

use common::{error, limited, line_width, opts, outputs, stderr, stdout, styled, texts};
use europa_core::notebook::UnknownOutput;
use europa_core::{Color, JsonMap, JsonValue, Output, Style, StyledLine};
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

/// The text `line 1\n` to `line {count}\n`.
fn numbered(count: usize) -> String {
    (1..=count).map(|n| format!("line {n}\n")).collect()
}

fn dim() -> Style {
    Style {
        dim: true,
        ..Style::default()
    }
}

#[test]
fn a_long_output_keeps_its_start_and_its_end_around_a_notice() {
    let cell = render_outputs(&[stdout(&numbered(20))], &limited(80, 10));
    assert_eq!(
        texts(&cell),
        [
            "line 1",
            "line 2",
            "line 3",
            "line 4",
            "... 11 lines hidden ...",
            "line 16",
            "line 17",
            "line 18",
            "line 19",
            "line 20",
        ]
    );
    assert_eq!(
        styled(&cell.lines[4]),
        [("... 11 lines hidden ...".to_owned(), dim())]
    );
    assert_eq!(cell.lines[4].fill, None);
}

#[test]
fn an_output_within_the_limit_has_no_notice() {
    for count in [5, 10] {
        let cell = render_outputs(&[stdout(&numbered(count))], &limited(80, 10));
        assert_eq!(cell.lines.len(), count);
        assert!(
            texts(&cell).iter().all(|line| line.starts_with("line ")),
            "{count} lines"
        );
    }
}

#[test]
fn the_limit_counts_the_lines_of_all_outputs_together() {
    let cell = render_outputs(
        &[
            stdout("o1\no2\no3\no4\no5\no6\n"),
            error("ValueError", "bad", &["t1", "t2", "t3", "t4", "t5", "t6"]),
        ],
        &limited(80, 10),
    );
    assert_eq!(
        texts(&cell),
        [
            "o1",
            "o2",
            "o3",
            "o4",
            "... 3 lines hidden ...",
            "t2",
            "t3",
            "t4",
            "t5",
            "t6"
        ]
    );
    let fills: Vec<_> = cell.lines.iter().map(|line| line.fill).collect();
    let red = Some(Color::Indexed(52));
    assert_eq!(
        fills,
        [None, None, None, None, None, red, red, red, red, red]
    );
}

#[test]
fn a_limit_of_one_or_two_leaves_the_notice_and_at_most_the_last_line() {
    let body = stdout("l1\nl2\nl3\n");
    let one = render_outputs(std::slice::from_ref(&body), &limited(80, 1));
    assert_eq!(texts(&one), ["... 3 lines hidden ..."]);
    let two = render_outputs(std::slice::from_ref(&body), &limited(80, 2));
    assert_eq!(texts(&two), ["... 2 lines hidden ...", "l3"]);
}

#[test]
fn wrapped_lines_count_against_the_limit() {
    let cell = render_outputs(&[stdout(&"x".repeat(300))], &limited(20, 10));
    assert_eq!(cell.lines.len(), 10);
    // The notice is 22 columns wide and is cut to the 20 of the area.
    assert_eq!(texts(&cell)[4], "... 6 lines hidden .");
}

#[test]
fn a_notice_wider_than_the_area_is_cut_and_stays_one_line() {
    let cell = render_outputs(&[stdout(&numbered(20))], &limited(10, 10));
    assert_eq!(cell.lines.len(), 10);
    assert_eq!(texts(&cell)[4], "... 11 lin");
}

#[test]
fn the_end_of_a_cut_output_keeps_the_style_it_had() {
    let body = format!("\u{1b}[31m{}", numbered(20));
    let cell = render_outputs(&[stdout(&body)], &limited(80, 10));
    for line in &cell.lines[5..] {
        assert_eq!(line.spans[0].style.fg, Some(Color::Indexed(1)));
    }
}

#[test]
fn a_style_set_in_the_hidden_lines_reaches_the_end() {
    let mut body = String::new();
    for n in 1..=20 {
        if n == 8 {
            body.push_str("\u{1b}[32m");
        }
        body.push_str(&format!("line {n}\n"));
    }
    let cell = render_outputs(&[stdout(&body)], &limited(80, 10));
    for line in &cell.lines[..4] {
        assert_eq!(line.spans[0].style, Style::default());
    }
    for line in &cell.lines[5..] {
        assert_eq!(line.spans[0].style.fg, Some(Color::Indexed(2)));
    }
}

#[test]
fn the_default_limit_is_two_hundred_lines() {
    let cell = render_outputs(&[stdout(&numbered(250))], &opts(80));
    assert_eq!(cell.lines.len(), 200);
    assert_eq!(texts(&cell)[99], "... 51 lines hidden ...");
}

#[test]
fn a_hundred_thousand_lines_still_fit_in_the_limit() {
    let output = stdout(&"x\n".repeat(100_000));
    let output = std::slice::from_ref(&output);
    assert_eq!(render_outputs(output, &opts(80)).lines.len(), 200);
    assert_eq!(render_outputs(output, &limited(80, 10)).lines.len(), 10);
}

/// A display data output with `data`, a JSON object.
fn display(data: &str) -> Vec<Output> {
    outputs(&format!(
        r#"[{{"output_type":"display_data","data":{data},"metadata":{{}}}}]"#
    ))
}

/// Display lines of `outputs` in an 80-column area.
fn shown(outputs: &[Output]) -> Vec<String> {
    texts(&render_outputs(outputs, &opts(80)))
}

#[test]
fn an_image_is_named_by_its_type_and_its_data_is_not_shown() {
    let cell = render_outputs(&display(r#"{"image/png":"iVBORw0KGgo="}"#), &opts(80));
    assert_eq!(texts(&cell), ["[image/png]"]);
    assert!(!texts(&cell).concat().contains("iVBOR"));
}

#[test]
fn all_the_types_a_result_has_are_named_in_order() {
    let result = outputs(
        r#"[{"output_type":"execute_result","data":{"text/html":"<b>x</b>","image/png":"iVBORw0KGgo="},"metadata":{},"execution_count":3}]"#,
    );
    assert_eq!(shown(&result), ["[image/png, text/html]"]);
}

#[test]
fn a_bundle_without_types_is_named_as_empty() {
    assert_eq!(shown(&display("{}")), ["[no data]"]);
}

#[test]
fn plain_text_that_is_not_text_is_named_like_any_other_type() {
    assert_eq!(
        shown(&display(r#"{"image/png":"x","text/plain":42}"#)),
        ["[image/png, text/plain]"]
    );
    assert_eq!(
        shown(&display(r#"{"text/plain":["a",1]}"#)),
        ["[text/plain]"]
    );
}

#[test]
fn an_unknown_output_is_named_by_its_kind_between_the_outputs_around_it() {
    let mut list = vec![stdout("a\n")];
    list.extend(outputs(r#"[{"output_type":"widget_state","state":{}}]"#));
    list.push(stdout("b\n"));
    assert_eq!(shown(&list), ["a", "[unknown output: widget_state]", "b"]);
}

#[test]
fn an_unknown_output_without_a_text_kind_is_named_without_one() {
    // A file never loads like this, but the type allows it.
    let bare = Output::Unknown(UnknownOutput {
        value: JsonMap::new(),
    });
    let odd = Output::Unknown(UnknownOutput {
        value: JsonMap::from([("output_type".to_owned(), JsonValue::Bool(true))]),
    });
    assert_eq!(
        shown(&[bare, odd]),
        ["[unknown output]", "[unknown output]"]
    );
}

#[test]
fn a_type_name_cannot_carry_an_escape_sequence_to_the_terminal() {
    let cell = render_outputs(&display(r#"{"image/\u001b[2Jpng":"x"}"#), &opts(80));
    assert_eq!(texts(&cell), ["[image/png]"]);
    assert!(cell
        .lines
        .iter()
        .flat_map(|line| &line.spans)
        .all(|span| !span.text.contains('\u{1b}')));
}

#[test]
fn a_notice_is_dim_and_not_filled_even_between_stderr_outputs() {
    let mut list = vec![stderr("e\n")];
    list.extend(display(r#"{"image/png":"x"}"#));
    list.push(stderr("f\n"));
    let cell = render_outputs(&list, &opts(80));
    assert_eq!(texts(&cell), ["e", "[image/png]", "f"]);
    assert_eq!(
        styled(&cell.lines[1]),
        [(
            "[image/png]".to_owned(),
            Style {
                dim: true,
                ..Style::default()
            }
        )]
    );
    assert_eq!(cell.lines[1].fill, None);
    assert!(cell.lines[0].fill.is_some() && cell.lines[2].fill.is_some());
}

#[test]
fn a_notice_wider_than_the_area_wraps_and_counts_against_the_limit() {
    let result = outputs(
        r#"[{"output_type":"execute_result","data":{"image/png":"x","text/html":"y"},"metadata":{},"execution_count":1}]"#,
    );
    let wrapped = render_outputs(&result, &opts(10));
    assert_eq!(texts(&wrapped), ["[image/png", ", text/htm", "l]"]);
    let cut = render_outputs(&result, &limited(10, 2));
    assert_eq!(texts(&cut), ["... 2 line", "l]"]);
}
