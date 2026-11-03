//! Highlighting of Python source with tree-sitter.

use std::ops::Range;
use std::sync::OnceLock;

use tree_sitter_highlight::{HighlightConfiguration, HighlightEvent, Highlighter};

/// Kind of a token, which decides how it is decorated.
///
/// The first eleven variants match [`CAPTURES`] one to one. `Magic` is not a
/// capture of the query. The pictures for control characters are not a kind
/// either; they are decorated after the highlighting, in `render_source`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    /// Keywords, including those that depend on context.
    Keyword,
    /// Built-in constants such as `None` and `True`.
    ConstantBuiltin,
    /// String content, prefixes and quotes.
    String,
    /// Escape sequences in strings.
    StringEscape,
    /// Comments.
    Comment,
    /// Integer and float literals.
    Number,
    /// Names given to functions and classes at their definition.
    FunctionDefinition,
    /// Calls of built-in functions.
    FunctionBuiltin,
    /// Decorators.
    Decorator,
    /// The braces of an f-string replacement field, its `=`, its conversion
    /// such as `!r` and its format specifier such as `:>10`.
    FStringSyntax,
    /// A replacement field of an f-string, which is Python code again.
    Embedded,
    /// Reserved for IPython magics and shell escapes, which tree-sitter
    /// cannot parse, so nothing produces it yet.
    #[expect(dead_code)]
    Magic,
}

/// Capture names of the query and their kinds.
///
/// The highlighter reports a capture by its index in this table, so the names
/// and the kinds sit side by side to keep that index meaning one kind.
const CAPTURES: [(&str, Kind); 11] = [
    ("keyword", Kind::Keyword),
    ("constant.builtin", Kind::ConstantBuiltin),
    ("string", Kind::String),
    ("string.escape", Kind::StringEscape),
    ("comment", Kind::Comment),
    ("number", Kind::Number),
    ("function.definition", Kind::FunctionDefinition),
    ("function.builtin", Kind::FunctionBuiltin),
    ("decorator", Kind::Decorator),
    ("punctuation.special", Kind::FStringSyntax),
    ("embedded", Kind::Embedded),
];

/// The configuration of the highlighter, which is built once.
///
/// It is `None` if the query does not build, and then nothing is decorated.
fn config() -> Option<&'static HighlightConfiguration> {
    static CONFIG: OnceLock<Option<HighlightConfiguration>> = OnceLock::new();
    CONFIG
        .get_or_init(|| {
            let mut config = HighlightConfiguration::new(
                tree_sitter_python::LANGUAGE.into(),
                "python",
                include_str!("../queries/python-highlights.scm"),
                "",
                "",
            )
            .ok()?;
            config.configure(&CAPTURES.map(|(name, _)| name));
            Some(config)
        })
        .as_ref()
}

/// Splits `source` into ranges of one kind each.
///
/// The ranges do not overlap and cover the whole source from the start
/// without gaps, and neighbours differ in kind. A source that is empty has no
/// range.
pub(crate) fn highlight(source: &str) -> Vec<(Range<usize>, Option<Kind>)> {
    if source.is_empty() {
        return Vec::new();
    }
    // Without a highlight the source still has to be shown whole, so a
    // failure of the highlighter must not cost any of its characters.
    let undecorated = || vec![(0..source.len(), None)];
    let Some(config) = config() else {
        return undecorated();
    };
    let mut highlighter = Highlighter::new();
    let Ok(events) = highlighter.highlight(config, source.as_bytes(), None, None, |_| None) else {
        return undecorated();
    };

    let mut stack: Vec<Kind> = Vec::new();
    let mut ranges: Vec<(Range<usize>, Option<Kind>)> = Vec::new();
    for event in events {
        match event {
            Err(_) => return undecorated(),
            Ok(HighlightEvent::HighlightStart(highlight)) => match CAPTURES.get(highlight.0) {
                Some((_, kind)) => stack.push(*kind),
                None => return undecorated(),
            },
            Ok(HighlightEvent::HighlightEnd) => {
                stack.pop();
            }
            Ok(HighlightEvent::Source { start, end }) if start < end => {
                let kind = stack.last().copied();
                match ranges.last_mut() {
                    Some((range, last)) if *last == kind && range.end == start => range.end = end,
                    _ => ranges.push((start..end, kind)),
                }
            }
            Ok(HighlightEvent::Source { .. }) => {}
        }
    }
    debug_assert_eq!(ranges.first().map(|(range, _)| range.start), Some(0));
    debug_assert_eq!(
        ranges.last().map(|(range, _)| range.end),
        Some(source.len())
    );
    debug_assert!(ranges
        .windows(2)
        .all(|pair| pair[0].0.end == pair[1].0.start));
    debug_assert!(ranges.iter().all(
        |(range, _)| source.is_char_boundary(range.start) && source.is_char_boundary(range.end)
    ));
    ranges
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ranges_cover_the_source() {
        for source in [
            "",
            "x = 1\n",
            "f\"{d[\"key\"]}\"\n",
            "def f(:\n",
            "\"\"\"abc",
        ] {
            let ranges = highlight(source);
            if source.is_empty() {
                assert!(ranges.is_empty());
                continue;
            }
            let mut end = 0;
            for (range, _) in &ranges {
                assert!(range.start < range.end, "empty range in {source:?}");
                assert_eq!(range.start, end, "gap or overlap in {source:?}");
                end = range.end;
            }
            assert_eq!(end, source.len(), "{source:?} is not covered");
            for pair in ranges.windows(2) {
                assert_ne!(pair[0].1, pair[1].1, "neighbours of one kind in {source:?}");
            }
        }
    }

    #[test]
    fn the_configuration_builds() {
        assert!(config().is_some());
    }

    #[test]
    fn the_query_captures_exactly_the_names_of_the_table() {
        let config = config().expect("configuration builds");
        let in_query: std::collections::BTreeSet<&str> =
            config.query.capture_names().iter().copied().collect();
        let in_table: std::collections::BTreeSet<&str> =
            CAPTURES.iter().map(|(name, _)| *name).collect();
        assert_eq!(in_query, in_table);
    }

    #[test]
    fn a_string_in_a_replacement_field_stays_in_one_string() {
        let source = "f\"{d[\"key\"]}\"\n";
        let mut parser = tree_sitter::Parser::new();
        parser
            .set_language(&tree_sitter_python::LANGUAGE.into())
            .expect("language loads");
        let tree = parser.parse(source, None).expect("parses");
        let root = tree.root_node();
        assert!(!root.has_error(), "{}", root.to_sexp());
        let statement = root.child(0).expect("a statement");
        let string = statement.child(0).expect("a string");
        assert_eq!(string.kind(), "string", "{}", root.to_sexp());
        assert_eq!(
            string.end_byte(),
            source.trim_end().len(),
            "{}",
            root.to_sexp()
        );
        assert!(
            root.to_sexp().contains("subscript: (string"),
            "{}",
            root.to_sexp()
        );
    }

    /// The capture that the highlighter gives `x` with the two patterns in the
    /// given order, as an index into `["a", "b"]`.
    fn last_pattern_wins(query: &str) -> Option<usize> {
        let mut config = HighlightConfiguration::new(
            tree_sitter_python::LANGUAGE.into(),
            "python",
            query,
            "",
            "",
        )
        .expect("query builds");
        config.configure(&["a", "b"]);
        let mut highlighter = Highlighter::new();
        let events = highlighter
            .highlight(&config, b"x", None, None, |_| None)
            .expect("highlights");
        for event in events {
            if let HighlightEvent::HighlightStart(highlight) = event.expect("event") {
                return Some(highlight.0);
            }
        }
        None
    }

    #[test]
    fn the_last_pattern_of_a_node_wins() {
        assert_eq!(
            last_pattern_wins("((identifier) @a (#eq? @a \"x\"))\n(identifier) @b"),
            Some(1)
        );
        assert_eq!(
            last_pattern_wins("(identifier) @b\n((identifier) @a (#eq? @a \"x\"))"),
            Some(0)
        );
    }
}
