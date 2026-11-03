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

const KEYWORD: Style = fg(5, false);
const CONSTANT: Style = fg(5, true);
const STRING: Style = fg(2, false);
const ESCAPE: Style = fg(2, true);
const COMMENT: Style = Style { dim: true, ..BASE };
const NUMBER: Style = fg(6, false);
const DEFINITION: Style = fg(4, true);
const BUILTIN: Style = fg(4, false);
const DECORATOR: Style = fg(3, true);
const FSTRING: Style = fg(3, false);
const MAGIC: Style = fg(6, true);
const CONTROL: Style = fg(1, true);
const PLAIN: Style = BASE;

const fn fg(index: u8, bold: bool) -> Style {
    Style {
        fg: Some(Color::Indexed(index)),
        bold,
        ..BASE
    }
}

fn python(source: &str) -> Vec<europa_core::StyledLine> {
    let lines = render_source(source, SourceLanguage::Python, &opts(80));
    assert_source_lines_are_well_formed(&lines);
    lines
}

/// Check that every character of `needle`, where it first occurs, has `style`.
#[track_caller]
fn assert_styled(lines: &[europa_core::StyledLine], needle: &str, style: Style) {
    let styles = styles_of(lines, needle);
    assert!(
        styles.iter().all(|found| *found == style),
        "{needle:?} is {styles:?}, not {style:?}"
    );
}

#[test]
fn keywords_numbers_comments_and_definitions() {
    let lines = python("def f(x):\n    return x + 1  # inc");
    assert_styled(&lines, "def", KEYWORD);
    assert_styled(&lines, "return", KEYWORD);
    assert_styled(&lines, "1", NUMBER);
    assert_styled(&lines, "# inc", COMMENT);
    assert_eq!(styles_of(&lines, "f(x)")[0], DEFINITION);
    assert_styled(&lines, "(x)", PLAIN);
    assert_styled(&lines, "x + ", PLAIN);
}

#[test]
fn strings_include_prefixes_and_quotes() {
    for source in [
        "'single'",
        "\"double\"",
        "b\"x\"",
        "rb'x'",
        "r\"\\d\"",
        "r\"\\n\"",
    ] {
        assert_styled(&python(source), source, STRING);
    }
    let lines = python("\"\"\"triple\nquoted\"\"\"");
    assert_styled(&lines, "\"\"\"triple", STRING);
    assert_styled(&lines, "quoted\"\"\"", STRING);
}

#[test]
fn escapes_are_bold_in_strings() {
    let lines = python("\"a\\nb\"");
    assert_styled(&lines, "\\n", ESCAPE);
    assert_styled(&lines, "\"a", STRING);
}

#[test]
fn builtin_constants() {
    for source in ["True", "False", "None"] {
        assert_styled(&python(source), source, CONSTANT);
    }
}

#[test]
fn decorators_are_marked_without_their_arguments() {
    assert_styled(&python("@decorator"), "@decorator", DECORATOR);
    assert_styled(&python("@functools.cache"), "@functools.cache", DECORATOR);
    let lines = python("@lru_cache(maxsize=2)\ndef g(): pass");
    assert_styled(&lines, "@lru_cache", DECORATOR);
    assert_styled(&lines, "maxsize=", PLAIN);
    assert_styled(&lines, "2", NUMBER);
}

#[test]
fn builtin_calls_and_plain_names() {
    assert_styled(&python("import os"), "import", KEYWORD);
    let lines = python("print(len(x))");
    assert_styled(&lines, "print", BUILTIN);
    assert_styled(&lines, "len", BUILTIN);
    assert_styled(&lines, "x", PLAIN);
    assert_styled(&python("f(x)"), "f(x)", PLAIN);
}

#[test]
fn class_names_are_definitions() {
    assert_styled(&python("class A:\n    pass"), "A", DEFINITION);
}

#[test]
fn word_keywords() {
    for source in ["and", "or", "in", "is", "is not", "not in"] {
        assert_styled(&python(&format!("x = a {source} b")), source, KEYWORD);
    }
    assert_styled(&python("async def f(): await g()"), "async", KEYWORD);
    assert_styled(&python("async def f(): await g()"), "await", KEYWORD);
    assert_styled(&python("f = lambda: 1"), "lambda", KEYWORD);
    assert_styled(&python("def f(): yield 1"), "yield", KEYWORD);
    assert_styled(&python("x = not y"), "not", KEYWORD);
}

#[test]
fn print_as_a_name_is_plain() {
    assert_styled(&python("print = 1"), "print", PLAIN);
}

#[test]
fn soft_keywords_depend_on_position() {
    let lines = python("match x:\n    case 1:\n        pass");
    assert_styled(&lines, "match", KEYWORD);
    assert_styled(&lines, "case", KEYWORD);
    assert_styled(&python("match = 1"), "match", PLAIN);
    let lines = python("type Point = tuple[int, int]");
    assert_styled(&lines, "type", KEYWORD);
    assert_styled(&python("type(x)"), "type", BUILTIN);
}

#[test]
fn docstrings_are_strings() {
    assert_styled(
        &python("def f():\n    \"\"\"Doc.\"\"\"\n"),
        "\"\"\"Doc.\"\"\"",
        STRING,
    );
}

#[test]
fn the_decorations_of_all_kinds_differ() {
    let all = [
        KEYWORD, CONSTANT, STRING, ESCAPE, COMMENT, NUMBER, DEFINITION, BUILTIN, DECORATOR,
        FSTRING, MAGIC, CONTROL,
    ];
    for (index, style) in all.iter().enumerate() {
        assert!(!all[index + 1..].contains(style), "{style:?} is not unique");
    }
}

#[test]
fn wrapped_lines_keep_the_decoration() {
    let lines = render_source("x = \"0123456789abc\"", SourceLanguage::Python, &opts(10));
    assert_eq!(lines.len(), 2);
    assert_styled(&lines, "abc\"", STRING);
    let lines = render_source("\"\"\"a\nb\"\"\"", SourceLanguage::Python, &opts(2));
    let text = source_lines(&lines);
    for line in &lines {
        assert!(
            line.spans.iter().all(|span| span.style == STRING),
            "{text:?}"
        );
    }
}

#[test]
fn control_characters_inside_strings_keep_their_own_decoration() {
    let lines = python("\"\u{1b}[31mred\"");
    assert_eq!(source_lines(&lines), ["\"␛[31mred\""]);
    assert_styled(&lines, "␛", CONTROL);
    assert_styled(&lines, "[31mred\"", STRING);
    assert_styled(&lines, "\"", STRING);
}

#[test]
fn a_nul_character_does_not_shift_the_decoration() {
    let lines = python("a = \"\u{0}\"\nc = \"x\"");
    assert_eq!(source_lines(&lines), ["a = \"␀\"", "c = \"x\""]);
    assert_styled(&lines, "a = ", PLAIN);
    let quoted = styles_of(&lines, "\"␀\"");
    assert_eq!(quoted, [STRING, CONTROL, STRING]);
    assert_styled(&lines, "c = ", PLAIN);
    assert_styled(&lines, "\"x\"", STRING);
}

#[test]
fn fstring_text_and_braces() {
    let lines = python("f\"total: {n + 1} items\"");
    assert_styled(&lines, "f\"total: ", STRING);
    assert_styled(&lines, " items\"", STRING);
    assert_styled(&lines, "{", FSTRING);
    assert_styled(&lines, "}", FSTRING);
    assert_styled(&lines, "1", NUMBER);
    assert_styled(&lines, "n + ", PLAIN);
}

#[test]
fn fstring_format_specs_and_conversions() {
    let lines = python("f\"{value:.2f}\"");
    assert_styled(&lines, ":.2f", FSTRING);
    assert_styled(&lines, "value", PLAIN);
    let lines = python("f\"{x!r}\"");
    assert_styled(&lines, "!r", FSTRING);
    assert_styled(&lines, "x", PLAIN);
}

#[test]
fn fstring_inner_strings() {
    let lines = python("f\"{d['key']}\"");
    assert_styled(&lines, "'key'", STRING);
    assert_styled(&lines, "d[", PLAIN);
    assert_styled(&lines, "]", PLAIN);
}

#[test]
fn escaped_braces_stay_in_the_string() {
    assert_styled(&python("f\"{{literal}}\""), "f\"{{literal}}\"", STRING);
}

#[test]
fn the_same_quote_inside_a_replacement_field() {
    let same = python("f\"{d[\"key\"]}\"");
    let other = python("f\"{d['key']}\"");
    // The quotes differ in the two sources, and the decorations do not.
    let decorations = |lines: &[europa_core::StyledLine]| -> Vec<Style> {
        lines
            .iter()
            .flat_map(|line| line.spans.iter())
            .flat_map(|span| span.text.chars().map(|_| span.style))
            .collect()
    };
    assert_eq!(decorations(&same), decorations(&other));
    assert_styled(&same, "\"key\"", STRING);
    let last = styles_of(&same, "}\"");
    assert_eq!(last[1], STRING);
}

#[test]
fn nested_replacement_fields_and_strings() {
    let lines = python("f\"{x:{width}}\"");
    assert_styled(&lines, "width", PLAIN);
    let lines = python("f\"{f'{y}'}\"");
    assert_styled(&lines, "y", PLAIN);
    assert_styled(&lines, "f'", STRING);
    assert_eq!(styles_of(&lines, "}'")[1], STRING);
}

#[test]
fn raw_fstrings() {
    let lines = python("rf\"\\d{n}\"");
    assert_styled(&lines, "\\d", STRING);
    assert_styled(&lines, "n", PLAIN);
}

#[test]
fn debug_specifiers() {
    let lines = python("f\"{x=}\"");
    assert_styled(&lines, "=", FSTRING);
    assert_styled(&lines, "x", PLAIN);
    let lines = python("f\"{x = !r}\"");
    assert_styled(&lines, "=", FSTRING);
    assert_styled(&lines, "x ", PLAIN);
}

#[test]
fn replacement_fields_with_newlines_and_comments() {
    let lines = python("f\"\"\"{a +\n b}\"\"\"");
    assert_styled(&lines, "a +", PLAIN);
    assert_styled(&lines, "f\"\"\"", STRING);
    assert_styled(&lines, "\"\"\"", STRING);
    let lines = python("f\"\"\"{a # c\n}\"\"\"");
    assert_styled(&lines, "# c", COMMENT);
    assert_styled(&lines, "a ", PLAIN);
}

/// Check that no character of `source` is lost, which holds where it has no
/// tab, carriage return or control character.
#[track_caller]
fn assert_all_characters_shown(source: &str) {
    let lines = python(source);
    assert_eq!(
        source_lines(&lines).join("\n"),
        source.trim_end_matches('\n')
    );
}

#[test]
fn a_syntax_error_does_not_hide_the_rest() {
    let source = "def f(:\n    return 1";
    assert_all_characters_shown(source);
    let lines = python(source);
    assert_styled(&lines, "return", KEYWORD);
    assert_styled(&lines, "1", NUMBER);
}

#[test]
fn an_unclosed_triple_quote_shows_everything() {
    assert_all_characters_shown("\"\"\"abc");
    assert_all_characters_shown("x = 1\n\"\"\"abc\ndef");
}

#[test]
fn odd_fragments_are_shown() {
    for source in ["len?", "x = (", ")))", "def", &"(\n".repeat(100)] {
        assert_all_characters_shown(source);
    }
}

#[test]
fn decorations_use_only_palette_colors_and_no_background() {
    let source = "@deco\ndef f(x, y=None):\n    \"\"\"Doc \\n\"\"\"\n    return f\"{x!r:>{y}} \\t\" + print(1)  # c\n\u{1b}\n";
    let dark = render_source(source, SourceLanguage::Python, &opts(80));
    let light = render_source(source, SourceLanguage::Python, &light(80));
    assert_eq!(dark, light);
    for line in &dark {
        for span in &line.spans {
            let style = span.style;
            assert_eq!(style.bg, None);
            assert!(
                matches!(style.fg, None | Some(Color::Indexed(1..=6))),
                "{style:?}"
            );
            assert!(!style.italic && !style.underline && !style.reverse && !style.strikethrough);
        }
    }
}

#[test]
fn plain_source_has_no_decoration() {
    let lines = render_source("def f(): return 1", SourceLanguage::Plain, &opts(80));
    assert_source_lines_are_well_formed(&lines);
    for line in &lines {
        assert!(line.spans.iter().all(|span| span.style == PLAIN));
    }
}
