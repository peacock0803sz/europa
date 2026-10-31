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
snapshot!(json_nested, "json-nested");
snapshot!(json_scalars, "json-scalars");
snapshot!(json_truncate, "json-truncate");
snapshot!(html_sample, "html-sample");
snapshot!(html_broken, "html-broken");
snapshot!(mime_select, "mime-select");
snapshot!(markdown_sample, "markdown-sample");
snapshot!(markdown_code_dark, "markdown-code-dark");
snapshot!(markdown_code_light, "markdown-code-light");
snapshot!(markdown_table, "markdown-table");
snapshot!(markdown_table_narrow, "markdown-table-narrow");

/// Fixtures whose output is longer than their limit, so that a notice stands
/// in for lines and the text is not the whole output.
const CUT: [&str; 4] = [
    "truncate-stream",
    "truncate-across",
    "notice-narrow",
    "json-truncate",
];

/// Fixtures with rules, lists, tables or indents that change with width, so
/// unwrapped text differs from wrapped text.
const WIDTH_VARIES: [&str; 7] = [
    "html-sample",
    "mime-select",
    "markdown-sample",
    "markdown-code-dark",
    "markdown-code-light",
    "markdown-table",
    "markdown-table-narrow",
];

const FIXTURES: [&str; 24] = [
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
    "json-nested",
    "json-scalars",
    "json-truncate",
    "html-sample",
    "html-broken",
    "mime-select",
    "markdown-sample",
    "markdown-code-dark",
    "markdown-code-light",
    "markdown-table",
    "markdown-table-narrow",
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
    for name in FIXTURES
        .into_iter()
        .filter(|name| !CUT.contains(name) && !WIDTH_VARIES.contains(name))
    {
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

#[test]
fn rich_fixtures_show_no_tag_shapes() {
    for name in [
        "json-nested",
        "json-scalars",
        "json-truncate",
        "html-sample",
        "html-broken",
        "markdown-sample",
        "markdown-code-dark",
        "markdown-code-light",
        "markdown-table",
        "markdown-table-narrow",
    ] {
        let (options, outputs) = fixture(&format!("{name}.json"));
        let cell = render_outputs(&outputs, &options);
        for line in texts(&cell) {
            let mut chars = line.chars();
            while let Some(c) = chars.next() {
                if c != '<' {
                    continue;
                }
                match chars.next() {
                    Some('a'..='z') | Some('A'..='Z') | Some('/') | Some('!') => {
                        panic!("{name}: tag shape in {line:?}")
                    }
                    _ => {}
                }
            }
        }
    }
}

#[test]
fn mime_select_shows_bodies_not_default_reprs() {
    let (options, outputs) = fixture("mime-select.json");
    let cell = render_outputs(&outputs, &options);
    let lines = texts(&cell);
    let text = lines.join("\n");
    assert!(!text.contains("IPython.core.display.JSON"), "{text}");
    assert!(!text.contains("IPython.core.display.Markdown"), "{text}");
    assert_eq!(
        text.matches("IPython.core.display.HTML").count(),
        1,
        "{text}"
    );
    assert!(text.contains("\"a\""), "{text}");
    assert!(text.contains('x'), "{text}");
    assert!(text.contains("0  1"), "{text}");
    assert!(lines.iter().any(|line| line == "T"), "{text}");
}

#[test]
fn markdown_sample_hides_its_marks() {
    let (options, outputs) = fixture("markdown-sample.json");
    let text = texts(&render_outputs(&outputs, &options)).join("\n");
    for mark in ["#", "**", "`", "]("] {
        assert!(!text.contains(mark), "{mark:?} in {text}");
    }
}

#[test]
fn rich_fixtures_keep_well_formed_spans() {
    for name in [
        "json-nested",
        "json-scalars",
        "json-truncate",
        "html-sample",
        "html-broken",
        "mime-select",
        "markdown-sample",
        "markdown-code-dark",
        "markdown-code-light",
        "markdown-table",
        "markdown-table-narrow",
    ] {
        let (options, outputs) = fixture(&format!("{name}.json"));
        let cell = render_outputs(&outputs, &options);
        common::assert_lines_are_well_formed(&cell);
    }
}

#[test]
fn rendering_does_not_change_what_is_saved_for_mime_select() {
    let outputs_json = common::fixture_outputs_json("mime-select.json");
    let notebook = common::notebook(&outputs_json);
    let before = europa_core::serialize(&notebook);
    let (options, _) = fixture("mime-select.json");
    let _ = render_outputs(common::cell_outputs(&notebook), &options);
    let after = europa_core::serialize(&notebook);
    assert_eq!(before, after);
}
