//! Wrapping of a logical line at the width of the area to draw in.
//!
//! Widths come from the same Unicode tables that ratatui uses to place text,
//! so a line that fits here also fits on the screen it is drawn on.

use std::num::NonZeroU16;

use europa_core::{Style, StyledSpan};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

/// Columns between tab stops.
const TAB_STOP: usize = 8;

/// Break one logical line into display lines at most `width` columns wide.
///
/// A line is broken where the width is reached, even inside a word. A grapheme
/// cluster that does not fit in the columns that are left goes to the next
/// line, so a wide character or a base with its combining marks is never
/// split. A cluster wider than the whole width stays whole on a line of its
/// own. A logical line with no characters gives one empty display line.
pub(crate) fn wrap(runs: &[StyledSpan], width: NonZeroU16) -> Vec<Vec<StyledSpan>> {
    let text: String = runs.iter().map(|run| run.text.as_str()).collect();
    let mut lines = Lines::new(usize::from(width.get()));
    // Tabs are counted from the start of the logical line and not of the
    // display line, so that where a tab ends does not depend on the width.
    let mut column = 0;
    let mut run = 0;
    let mut run_end = runs.first().map_or(0, |first| first.text.len());
    for (start, cluster) in text.grapheme_indices(true) {
        // A cluster can reach over the end of a run, and then it keeps the
        // style it began with.
        while start >= run_end {
            run += 1;
            run_end += runs[run].text.len();
        }
        let style = runs[run].style;
        if cluster == "\t" {
            let spaces = TAB_STOP - column % TAB_STOP;
            for _ in 0..spaces {
                lines.put(" ", 1, style);
            }
            column += spaces;
        } else {
            let columns = UnicodeWidthStr::width(cluster);
            lines.put(cluster, columns, style);
            column += columns;
        }
    }
    lines.finish()
}

struct Lines {
    width: usize,
    done: Vec<Vec<StyledSpan>>,
    line: Vec<StyledSpan>,
    used: usize,
}

impl Lines {
    fn new(width: usize) -> Self {
        Self {
            width,
            done: Vec::new(),
            line: Vec::new(),
            used: 0,
        }
    }

    /// Add a cluster that takes `columns` columns to the current line, or to
    /// a new one when it does not fit.
    fn put(&mut self, cluster: &str, columns: usize, style: Style) {
        // A cluster of no width never starts a line, and any cluster is placed
        // on an empty line however wide it is, or it would never be placed.
        if columns > 0 && self.used > 0 && self.used + columns > self.width {
            self.done.push(std::mem::take(&mut self.line));
            self.used = 0;
        }
        match self.line.last_mut() {
            Some(last) if last.style == style => last.text.push_str(cluster),
            _ => self.line.push(StyledSpan {
                text: cluster.to_owned(),
                style,
            }),
        }
        self.used += columns;
    }

    fn finish(mut self) -> Vec<Vec<StyledSpan>> {
        // A line that is exactly full starts no new line until a cluster
        // comes that needs one.
        if !self.line.is_empty() || self.done.is_empty() {
            self.done.push(self.line);
        }
        self.done
    }
}
