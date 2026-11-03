//! Finding IPython magics and shell escapes in the source of a cell.
//!
//! IPython rewrites them before Python reads the code, so tree-sitter cannot
//! parse them. They are found here by a scan that follows strings, brackets
//! and line continuations, because a `%` or a `!` only starts a magic at the
//! beginning of a logical line or after the first `=` of one.

use std::ops::Range;

/// Names of the cell magics whose body is Python.
const PYTHON_CELL_MAGICS: [&str; 4] = ["time", "timeit", "capture", "prun"];

/// What the scan of a cell found.
pub(crate) struct Magics {
    /// Byte ranges to decorate as magics, in ascending order, never
    /// overlapping, and never reaching over a line feed.
    pub(crate) ranges: Vec<Range<usize>>,
    /// The bytes to read as Python: the whole source, the lines after a cell
    /// magic whose body is Python, or nothing after any other cell magic.
    pub(crate) python: Range<usize>,
    /// The source with every byte of `ranges` replaced by `_`, which is what
    /// tree-sitter reads. A range then is one identifier, so the code around
    /// it keeps its structure and the bytes stay where they were.
    pub(crate) masked: String,
}

/// A place where the scan is, inside a string or in code.
enum Context {
    /// Code, with the depth of the brackets opened in it. `field` is set for
    /// the replacement field of an f-string, which ends at its closing brace.
    Code { depth: usize, field: bool },
    /// A string with its quote, whether it is triple-quoted, raw or an
    /// f-string.
    Str {
        quote: u8,
        triple: bool,
        raw: bool,
        format: bool,
    },
    /// The format spec after the `:` of a replacement field, which is text
    /// that can hold replacement fields of its own and ends with the field.
    Spec,
}

/// Finds the magics of `source`.
///
/// Nothing in the source can make this fail: a string or bracket that is not
/// closed lasts to the end of the source.
pub(crate) fn scan(source: &str) -> Magics {
    let bytes = source.as_bytes();
    let mut ranges = Vec::new();
    let mut python = 0..source.len();

    if source.starts_with("%%") {
        let end = line_end(bytes, 0);
        ranges.push(0..end);
        let name = source[2..end].split_whitespace().next().unwrap_or_default();
        python = if PYTHON_CELL_MAGICS.contains(&name) {
            (end + 1).min(source.len())..source.len()
        } else {
            source.len()..source.len()
        };
    }
    scan_python(bytes, python.clone(), &mut ranges);

    let mut masked = bytes.to_vec();
    for range in &ranges {
        masked[range.clone()].fill(b'_');
    }
    // The ranges start at an ASCII `%` or `!` and stop at a line feed or the
    // end, and every byte of them becomes ASCII, so this is valid UTF-8.
    let masked = String::from_utf8(masked);
    debug_assert!(masked.is_ok());
    let masked = masked.unwrap_or_else(|_| source.to_owned());
    Magics {
        ranges,
        python,
        masked,
    }
}

/// Scans `within` of `bytes` for the magics of logical lines.
fn scan_python(bytes: &[u8], within: Range<usize>, ranges: &mut Vec<Range<usize>>) {
    let end = within.end;
    let mut stack = vec![Context::Code {
        depth: 0,
        field: false,
    }];
    let mut at_line_start = true;
    let mut continued = false;
    let mut seen_equals = false;
    let mut i = within.start;

    while i < end {
        let top_level = stack.len() == 1 && matches!(stack[0], Context::Code { depth: 0, .. });

        if at_line_start {
            at_line_start = false;
            if top_level && !continued {
                seen_equals = false;
                let mut start = i;
                while start < end && matches!(bytes[start], b' ' | b'\t' | b'\x0c') {
                    start += 1;
                }
                if start < end && matches!(bytes[start], b'%' | b'!') {
                    let stop = line_end(bytes, start).min(end);
                    ranges.push(start..stop);
                    i = stop;
                    continue;
                }
            }
            continued = false;
        }

        let byte = bytes[i];
        match stack.last_mut() {
            Some(Context::Str {
                quote,
                triple,
                raw,
                format,
            }) => {
                let (quote, triple, raw, format) = (*quote, *triple, *raw, *format);
                match byte {
                    b'\n' => {
                        // Python ends a string that is not triple-quoted at
                        // the line feed, so a draft that leaves one open does
                        // not swallow the lines after it.
                        if !triple {
                            stack.pop();
                        }
                        at_line_start = true;
                        i += 1;
                    }
                    b'\\' => {
                        let next = bytes.get(i + 1).copied().filter(|_| i + 1 < end);
                        match next {
                            // A backslash can hide a quote or a backslash
                            // even in a raw string, and a line feed.
                            Some(b'\\' | b'\'' | b'"') => i += 2,
                            Some(b'\n') => {
                                at_line_start = true;
                                i += 2;
                            }
                            // `\N{name}` is not a replacement field.
                            Some(b'N') if !raw && format && bytes.get(i + 2) == Some(&b'{') => {
                                i += 3;
                                while i < end && bytes[i] != b'}' && bytes[i] != b'\n' {
                                    i += 1;
                                }
                                i = (i + 1).min(end);
                            }
                            _ => i += 1,
                        }
                    }
                    b if b == quote => {
                        if !triple {
                            stack.pop();
                            i += 1;
                        } else if bytes[i..end].starts_with(&[quote, quote, quote]) {
                            stack.pop();
                            i += 3;
                        } else {
                            i += 1;
                        }
                    }
                    b'{' if format => {
                        if bytes.get(i + 1) == Some(&b'{') && i + 1 < end {
                            i += 2;
                        } else {
                            stack.push(Context::Code {
                                depth: 0,
                                field: true,
                            });
                            i += 1;
                        }
                    }
                    b'}' if format && bytes.get(i + 1) == Some(&b'}') && i + 1 < end => i += 2,
                    _ => i += 1,
                }
            }
            Some(Context::Code { depth, field }) => match byte {
                b'\n' => {
                    at_line_start = true;
                    i += 1;
                }
                b'\\' if bytes.get(i + 1) == Some(&b'\n') && i + 1 < end => {
                    continued = true;
                    at_line_start = true;
                    i += 2;
                }
                b'#' => i = line_end(bytes, i).min(end),
                b'\'' | b'"' => {
                    let (raw, format) = string_prefix(bytes, within.start, i);
                    let triple = i + 2 < end && bytes[i + 1] == byte && bytes[i + 2] == byte;
                    stack.push(Context::Str {
                        quote: byte,
                        triple,
                        raw,
                        format,
                    });
                    i += if triple { 3 } else { 1 };
                }
                b'(' | b'[' | b'{' => {
                    *depth += 1;
                    i += 1;
                }
                b'}' if *field && *depth == 0 => {
                    stack.pop();
                    i += 1;
                }
                // Python reads a `:` outside brackets in a field as the start
                // of the format spec, even in `:=`.
                b':' if *field && *depth == 0 => {
                    stack.push(Context::Spec);
                    i += 1;
                }
                b')' | b']' | b'}' => {
                    *depth = depth.saturating_sub(1);
                    i += 1;
                }
                b'=' if top_level && !seen_equals && is_single_equals(bytes, i, end) => {
                    seen_equals = true;
                    let mut start = i + 1;
                    while start < end && matches!(bytes[start], b' ' | b'\t') {
                        start += 1;
                    }
                    if start < end && matches!(bytes[start], b'%' | b'!') {
                        let stop = line_end(bytes, start).min(end);
                        ranges.push(start..stop);
                        i = stop;
                    } else {
                        i += 1;
                    }
                }
                _ => i += 1,
            },
            // A `#` or a quote in a spec is text, as in `{n:#x}`.
            Some(Context::Spec) => match byte {
                b'\n' => {
                    at_line_start = true;
                    i += 1;
                }
                b'{' => {
                    stack.push(Context::Code {
                        depth: 0,
                        field: true,
                    });
                    i += 1;
                }
                b'}' => {
                    // The brace ends the spec and the field it belongs to.
                    stack.pop();
                    stack.pop();
                    i += 1;
                }
                _ => i += 1,
            },
            None => break,
        }
    }
}

/// The index of the line feed that ends the line `from` is on, or the length.
fn line_end(bytes: &[u8], from: usize) -> usize {
    bytes[from..]
        .iter()
        .position(|&byte| byte == b'\n')
        .map_or(bytes.len(), |offset| from + offset)
}

/// Whether the `=` at `at` stands alone, and so is not part of `==`, `<=`,
/// `:=`, `+=` and the like.
fn is_single_equals(bytes: &[u8], at: usize, end: usize) -> bool {
    let before = at
        .checked_sub(1)
        .map(|index| bytes[index])
        .unwrap_or_default();
    let after = bytes.get(at + 1).copied().filter(|_| at + 1 < end);
    !b"=<>!:+-*/%&|^@".contains(&before) && after != Some(b'=')
}

/// Whether the quote at `quote_at` has a prefix that makes the string raw, and
/// one that lets it hold replacement fields.
fn string_prefix(bytes: &[u8], start: usize, quote_at: usize) -> (bool, bool) {
    let mut from = quote_at;
    while from > start && quote_at - from < 2 && b"rRbBfFtTuU".contains(&bytes[from - 1]) {
        from -= 1;
    }
    // Letters that run on from a name are not a prefix.
    if from > start && (bytes[from - 1].is_ascii_alphanumeric() || bytes[from - 1] == b'_') {
        return (false, false);
    }
    let prefix = &bytes[from..quote_at];
    let has = |letters: &[u8]| prefix.iter().any(|byte| letters.contains(byte));
    (has(b"rR"), has(b"fFtT"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The text of each range found in `source`.
    fn found(source: &str) -> Vec<&str> {
        scan(source)
            .ranges
            .into_iter()
            .map(|range| &source[range])
            .collect()
    }

    #[test]
    fn line_magics_run_to_the_end_of_the_line() {
        assert_eq!(
            found("%timeit f()\n!ls -l\nx = 1"),
            ["%timeit f()", "!ls -l"]
        );
        assert_eq!(found("if x:\n    %time f()\n"), ["%time f()"]);
    }

    #[test]
    fn the_first_equals_of_a_logical_line_can_lead_to_a_magic() {
        assert_eq!(found("x = %time f()"), ["%time f()"]);
        assert_eq!(found("x=!ls"), ["!ls"]);
        assert_eq!(found("y = a % b"), Vec::<&str>::new());
        assert_eq!(found("x != !y"), Vec::<&str>::new());
        assert_eq!(found("x == !y"), Vec::<&str>::new());
        assert_eq!(found("f(a=%x)"), Vec::<&str>::new());
        assert_eq!(found("a = b = !ls"), Vec::<&str>::new());
    }

    #[test]
    fn strings_brackets_and_continuations_hide_a_magic() {
        assert_eq!(found("\"\"\"\n%d items\n\"\"\""), Vec::<&str>::new());
        assert_eq!(found("print(1,\n  %x)"), Vec::<&str>::new());
        assert_eq!(found("x = 1 + \\\n!y"), Vec::<&str>::new());
        assert_eq!(found("f\"{\n%x}\""), Vec::<&str>::new());
        assert_eq!(found("t\"{\n%x}\""), Vec::<&str>::new());
        assert_eq!(found("# %time"), Vec::<&str>::new());
    }

    #[test]
    fn a_logical_line_after_a_closed_string_or_bracket_can_be_a_magic() {
        assert_eq!(found("s = '''\na\n'''\n%time f()"), ["%time f()"]);
        assert_eq!(found("f(\n)\n!ls"), ["!ls"]);
        assert_eq!(found("f\"{x}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("rf\"\\{x}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"\\N{DASH}{{x}}\"\n!ls"), ["!ls"]);
    }

    #[test]
    fn a_format_spec_is_text_and_not_code() {
        assert_eq!(found("print(f\"{n:#x}\")\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"{x!r:#>10}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"{x:'^10}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"{x:{w}}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"{x:{w}#}\"\n%time f()"), ["%time f()"]);
        assert_eq!(found("f\"{d[1:2]}\"\n%time f()"), ["%time f()"]);
    }

    #[test]
    fn cell_magics_decide_what_is_python() {
        let source = "%%timeit -n 10\nsorted(range(100))\n%time f()";
        let magics = scan(source);
        assert_eq!(
            &source[magics.python.clone()],
            "sorted(range(100))\n%time f()"
        );
        assert_eq!(
            magics
                .ranges
                .iter()
                .map(|range| &source[range.clone()])
                .collect::<Vec<_>>(),
            ["%%timeit -n 10", "%time f()"]
        );

        let source = "%%bash\n%time f()";
        let magics = scan(source);
        assert!(magics.python.is_empty());
        assert_eq!(found(source), ["%%bash"]);

        let magics = scan(" %%time\nx = 1");
        assert_eq!(magics.python, 0..magics.masked.len());
        assert_eq!(found(" %%time\nx = 1"), ["%%time"]);
    }

    #[test]
    fn masking_keeps_every_byte_in_place() {
        let source = "x = %time f()\né = 1";
        let magics = scan(source);
        assert_eq!(magics.masked.len(), source.len());
        assert_eq!(magics.masked, "x = _________\né = 1");
        let magics = scan("a = !ls é");
        assert_eq!(magics.masked.len(), "a = !ls é".len());
    }

    #[test]
    fn unclosed_strings_and_brackets_do_not_fail() {
        for source in ["\"\"\"abc\n%x", "x = (\n%x", "f\"{\n%x", ")))\n%x", "\\"] {
            let _ = scan(source);
        }
        assert_eq!(found(")))\n%x"), ["%x"]);
    }

    #[test]
    fn a_string_on_one_line_ends_at_the_line_feed() {
        assert_eq!(found("x = 'abc\n%time f()"), ["%time f()"]);
        assert_eq!(found("x = \"abc\n!ls"), ["!ls"]);
        assert_eq!(found("x = 'abc\\\n%time f()'"), Vec::<&str>::new());
        assert_eq!(found("x = '''abc\n%time f()"), Vec::<&str>::new());
    }
}
