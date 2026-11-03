mod common;

use europa_core::{Color, SourceLanguage, Style, StyledLine};
use europa_render::render_source;

use common::{
    assert_source_lines_are_well_formed, expected_source_text, opts, source_lines, styles_of,
};

const BASE: Style = Style {
    fg: None,
    bg: None,
    bold: false,
    dim: false,
    italic: false,
    underline: false,
    reverse: false,
    strikethrough: false,
};
const MAGIC: Style = Style {
    fg: Some(Color::Indexed(6)),
    bold: true,
    ..BASE
};
const KEYWORD: Style = Style {
    fg: Some(Color::Indexed(5)),
    ..BASE
};
const NUMBER: Style = Style {
    fg: Some(Color::Indexed(6)),
    ..BASE
};
const STRING: Style = Style {
    fg: Some(Color::Indexed(2)),
    ..BASE
};
const BUILTIN: Style = Style {
    fg: Some(Color::Indexed(4)),
    ..BASE
};
const COMMENT: Style = Style { dim: true, ..BASE };

/// Render `source` as Python and check what holds for every source.
fn render(source: &str) -> Vec<StyledLine> {
    let lines = render_source(source, SourceLanguage::Python, &opts(80));
    assert_source_lines_are_well_formed(&lines);
    assert_eq!(
        source_lines(&lines).concat(),
        expected_source_text(source.trim_end_matches('\n')),
        "{source:?}"
    );
    lines
}

/// Check that every character of `needle`, where it first occurs, has `style`.
#[track_caller]
fn assert_styled(lines: &[StyledLine], needle: &str, style: Style) {
    let styles = styles_of(lines, needle);
    assert!(
        styles.iter().all(|found| *found == style),
        "{needle:?} is {styles:?}, not {style:?}"
    );
}

#[test]
fn line_magics_and_shell_escapes_cover_the_whole_line() {
    let lines = render("%timeit sum(range(10))\n!pip list\nx = 1");
    assert_styled(&lines, "%timeit sum(range(10))", MAGIC);
    assert_styled(&lines, "!pip list", MAGIC);
    assert_eq!(styles_of(&lines, "x = 1")[4], NUMBER);
    assert_styled(&lines, "x = ", BASE);
}

#[test]
fn an_indented_magic_leaves_the_rest_of_the_block_alone() {
    let lines = render("if x:\n    %time f()\nelse:\n    y");
    assert_styled(&lines, "%time f()", MAGIC);
    assert_styled(&lines, "else", KEYWORD);
    assert_styled(&lines, "if", KEYWORD);
}

#[test]
fn the_right_hand_side_of_an_assignment_can_be_a_magic() {
    let lines = render("x = %time f()");
    assert_styled(&lines, "x = ", BASE);
    assert_styled(&lines, "%time f()", MAGIC);

    let lines = render("files = !ls");
    assert_styled(&lines, "files = ", BASE);
    assert_styled(&lines, "!ls", MAGIC);

    let lines = render("x=%time f()");
    assert_styled(&lines, "x=", BASE);
    assert_styled(&lines, "%time f()", MAGIC);
}

#[test]
fn operators_are_not_magics() {
    assert_styled(&render("y = a % b"), "y = a % b", BASE);
    assert_styled(&render("x != !y"), "x != !y", BASE);
}

#[test]
fn a_cell_magic_with_another_body_leaves_the_body_alone() {
    let lines = render("%%bash\necho $HOME");
    assert_styled(&lines, "%%bash", MAGIC);
    assert_styled(&lines, "echo $HOME", BASE);

    let lines = render("%%sql\nselect * from t");
    assert_styled(&lines, "%%sql", MAGIC);
    assert_styled(&lines, "select * from t", BASE);
}

#[test]
fn a_cell_magic_with_a_python_body_highlights_the_body() {
    for header in ["%%timeit -n 10", "%%time", "%%capture", "%%prun"] {
        let lines = render(&format!("{header}\nsorted(range(100))"));
        assert_styled(&lines, header, MAGIC);
        assert_styled(&lines, "sorted", BUILTIN);
        assert_styled(&lines, "100", NUMBER);
    }
}

#[test]
fn a_line_magic_inside_a_python_cell_magic_body() {
    let lines = render("%%capture\n%time f()\nx = 1");
    assert_styled(&lines, "%%capture", MAGIC);
    assert_styled(&lines, "%time f()", MAGIC);
    assert_eq!(styles_of(&lines, "x = 1")[4], NUMBER);
}

#[test]
fn a_cell_magic_cannot_follow_a_space() {
    let lines = render(" %%time\nx = 1");
    assert_styled(&lines, "%%time", MAGIC);
    assert_styled(&lines, " ", BASE);
    assert_eq!(styles_of(&lines, "x = 1")[4], NUMBER);
}

#[test]
fn a_percent_in_a_string_or_in_brackets_is_not_a_magic() {
    assert_styled(&render("\"\"\"\n%d items\n\"\"\""), "%d items", STRING);

    let lines = render("print(\"%s\"\n    % x)");
    assert_styled(&lines, "print", BUILTIN);
    assert_styled(&lines, "% x", BASE);
}

#[test]
fn a_line_continuation_does_not_start_a_logical_line() {
    let lines = render("x = 1 + \\\n!y");
    assert_styled(&lines, "!y", BASE);
}

#[test]
fn a_percent_in_a_replacement_field_is_not_a_magic() {
    for source in ["f\"{\n%x}\"", "t\"{\n%x}\""] {
        let lines = render(source);
        let styles = styles_of(&lines, "%x");
        assert!(styles.iter().all(|style| *style != MAGIC), "{source:?}");
    }
}

#[test]
fn a_format_spec_does_not_hide_a_later_magic() {
    let lines = render("print(f\"{n:#x}\")\n%matplotlib inline");
    assert_styled(&lines, "%matplotlib inline", MAGIC);
}

#[test]
fn a_comment_stays_a_comment() {
    assert_styled(&render("# %time"), "# %time", COMMENT);
}
