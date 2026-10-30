mod common;

use std::num::NonZeroU16;

use common::{fixture, texts};
use europa_core::RenderOptions;
use europa_render::render_outputs;

macro_rules! snapshot {
    ($name:ident, $file:literal) => {
        #[test]
        fn $name() {
            let (options, outputs) = fixture(concat!($file, ".json"));
            let cell = render_outputs(&outputs, &options);
            insta::assert_snapshot!(concat!("render-", $file), common::draw(&cell, &options));
        }
    };
}

snapshot!(ansi_styles, "ansi-styles");
snapshot!(escape_removal, "escape-removal");
snapshot!(stdout_stderr_dark, "stdout-stderr-dark");
snapshot!(stdout_stderr_light, "stdout-stderr-light");
snapshot!(text_plain, "text-plain");
snapshot!(truncate_stream, "truncate-stream");
snapshot!(truncate_across, "truncate-across");
snapshot!(wrap_wide, "wrap-wide");
snapshot!(traceback_ipython, "traceback-ipython");
snapshot!(traceback_edge, "traceback-edge");
snapshot!(carriage_return, "carriage-return");
snapshot!(placeholders, "placeholders");
snapshot!(notice_narrow, "notice-narrow");

/// Fixtures whose output is longer than their limit, so that a notice stands
/// in for lines and the text is not the whole output.
const CUT: [&str; 3] = ["truncate-stream", "truncate-across", "notice-narrow"];

const FIXTURES: [&str; 13] = [
    "ansi-styles",
    "escape-removal",
    "stdout-stderr-dark",
    "stdout-stderr-light",
    "text-plain",
    "truncate-stream",
    "truncate-across",
    "wrap-wide",
    "traceback-ipython",
    "traceback-edge",
    "carriage-return",
    "placeholders",
    "notice-narrow",
];

#[test]
fn no_fixture_leaves_a_control_character_in_a_line() {
    for name in FIXTURES {
        let (options, outputs) = fixture(&format!("{name}.json"));
        let cell = render_outputs(&outputs, &options);
        for span in cell.lines.iter().flat_map(|line| &line.spans) {
            assert!(
                !span.text.chars().any(char::is_control),
                "{name}: {:?}",
                span.text
            );
        }
    }
}

#[test]
fn the_width_does_not_change_the_text_of_a_fixture_that_is_not_cut() {
    for name in FIXTURES.into_iter().filter(|name| !CUT.contains(name)) {
        let (options, outputs) = fixture(&format!("{name}.json"));
        let as_given = texts(&render_outputs(&outputs, &options)).concat();
        let wide = RenderOptions {
            width: NonZeroU16::MAX,
            ..options
        };
        let unwrapped = texts(&render_outputs(&outputs, &wide)).concat();
        assert_eq!(as_given, unwrapped, "{name}");
    }
}

#[test]
fn the_recorded_traceback_shows_the_name_the_value_the_file_and_the_line() {
    let (options, outputs) = fixture("traceback-ipython.json");
    let text = texts(&render_outputs(&outputs, &options)).join("\n");
    for expected in [
        "NameError",
        "name 'iAmNotDefined' is not defined",
        "<ipython-input-22-56e1109ae320>",
        "----> 1",
    ] {
        assert!(
            text.contains(expected),
            "{expected:?} is missing from {text}"
        );
    }
}
