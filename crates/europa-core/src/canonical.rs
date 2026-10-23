//! Canonical forms for outputs that Europa builds.
//!
//! nbformat writes multiline text as arrays of lines. The functions here
//! decide where those lines break and which MIME values get the treatment.

/// Split text into lines the way Python's `str.splitlines(True)` does.
///
/// Breaks come after LF, CR, CRLF, U+000B, U+000C, U+001C, U+001D, U+001E,
/// U+0085, U+2028 and U+2029, and each element keeps its break. CRLF is one
/// break. Empty text gives no elements and a last line without a break is its
/// own element.
pub(crate) fn split_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut start = 0;
    let mut chars = text.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        let mut end = idx + ch.len_utf8();
        match ch {
            '\r' => {
                if let Some(&(next_idx, '\n')) = chars.peek() {
                    chars.next();
                    end = next_idx + 1;
                }
            }
            '\n' | '\u{0B}' | '\u{0C}' | '\u{1C}' | '\u{1D}' | '\u{1E}' | '\u{85}' | '\u{2028}'
            | '\u{2029}' => {}
            _ => continue,
        }
        lines.push(text[start..end].to_owned());
        start = end;
    }
    if start < text.len() {
        lines.push(text[start..].to_owned());
    }
    lines
}

/// Whether nbformat splits values of this MIME type into lines.
pub(crate) fn is_split_mime(mime: &str) -> bool {
    mime.starts_with("text/") || mime == "application/javascript" || mime == "image/svg+xml"
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ch(code: u32) -> char {
        char::from_u32(code).expect("valid scalar value")
    }

    #[test]
    fn splits_after_each_break_and_keeps_it() {
        for code in [
            0x0A, 0x0D, 0x0B, 0x0C, 0x1C, 0x1D, 0x1E, 0x85, 0x2028, 0x2029,
        ] {
            let sep = ch(code);
            let text = format!("a{sep}b");
            assert_eq!(split_lines(&text), vec![format!("a{sep}"), "b".to_owned()]);
        }
    }

    #[test]
    fn treats_crlf_as_one_break() {
        assert_eq!(split_lines("a\r\nb"), ["a\r\n", "b"]);
        assert_eq!(split_lines("a\r\r\nb"), ["a\r", "\r\n", "b"]);
    }

    #[test]
    fn handles_empty_and_unterminated_text() {
        assert!(split_lines("").is_empty());
        assert_eq!(split_lines("\n"), ["\n"]);
        assert_eq!(split_lines("tail"), ["tail"]);
        assert_eq!(split_lines("a\n"), ["a\n"]);
        assert_eq!(split_lines("\n\n"), ["\n", "\n"]);
    }

    #[test]
    fn pieces_join_back_and_split_the_same_way() {
        let text = format!(
            "lf\ncr\rcrlf\r\nvt{}ff{}fs{}gs{}rs{}nel{}ls{}ps{}tail",
            ch(0x0B),
            ch(0x0C),
            ch(0x1C),
            ch(0x1D),
            ch(0x1E),
            ch(0x85),
            ch(0x2028),
            ch(0x2029)
        );
        let pieces = split_lines(&text);
        assert_eq!(pieces.len(), 12);
        let joined = pieces.concat();
        assert_eq!(joined, text);
        assert_eq!(split_lines(&joined), pieces);
    }

    #[test]
    fn picks_the_mime_types_nbformat_splits() {
        for mime in [
            "text/plain",
            "text/html",
            "text/markdown",
            "text/latex",
            "text/x-custom+json",
            "application/javascript",
            "image/svg+xml",
        ] {
            assert!(is_split_mime(mime), "{mime}");
        }
        for mime in [
            "image/png",
            "image/jpeg",
            "application/json",
            "application/vnd.custom+json",
            "application/pdf",
            "application/javascriptx",
        ] {
            assert!(!is_split_mime(mime), "{mime}");
        }
    }
}
