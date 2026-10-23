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
#[allow(dead_code, unused_variables)]
pub(crate) fn split_lines(text: &str) -> Vec<String> {
    todo!()
}

/// Whether nbformat splits values of this MIME type into lines.
#[allow(dead_code, unused_variables)]
pub(crate) fn is_split_mime(mime: &str) -> bool {
    todo!()
}
