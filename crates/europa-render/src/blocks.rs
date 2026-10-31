//! Logical lines with indentation for Markdown and HTML.
//!
//! Converters return logical lines and leave wrapping to the shared wrapper,
//! so that continuation lines keep the indentation of their item or quote.

use std::num::NonZeroU16;

use europa_core::{Color, Style, StyledSpan};
use unicode_width::UnicodeWidthStr;

/// One logical line with its indentation.
///
/// `lead` opens the first display line and `hang` opens the continuation
/// lines. Both have the same width, so text stays aligned when it wraps.
/// An empty line has no body and no fill, and its `lead` and `hang` are the
/// hanging indentation of every level with trailing blanks removed. Only a
/// blank body inside a code block or a `pre` element keeps a fill.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Line {
    /// Indentation and marker of the first display line.
    pub(crate) lead: Vec<StyledSpan>,
    /// Indentation of the continuation display lines.
    pub(crate) hang: Vec<StyledSpan>,
    /// Body of the line, possibly empty.
    pub(crate) spans: Vec<StyledSpan>,
    /// Background of the whole line, if it is a code line.
    pub(crate) fill: Option<Color>,
}

/// One nesting level of an item or a quote.
struct Level {
    /// Marker shown once, before the first line of the level.
    lead: Vec<StyledSpan>,
    /// Indentation shown before every line of the level.
    hang: Vec<StyledSpan>,
    /// Whether the first line of the level was already emitted.
    lead_used: bool,
    /// Whether the level is an item rather than a quote.
    item: bool,
}

/// Assembly of logical lines with shared indentation and styling.
#[allow(dead_code)]
pub(crate) struct Blocks {
    /// Finished logical lines.
    lines: Vec<Line>,
    /// Body spans of the line being built.
    current: Vec<StyledSpan>,
    /// Whether to insert one blank line before the next content.
    gap: bool,
    /// Stack of enclosing items and quotes.
    levels: Vec<Level>,
    /// Stack of enclosing text decorations.
    styles: Vec<Style>,
    /// Background while inside a code block or a `pre` element.
    fill: Option<Color>,
    /// Width of the area to draw in.
    width: NonZeroU16,
}

#[allow(dead_code)]
impl Blocks {
    /// Empty assembly for an area `width` columns wide.
    fn new(width: NonZeroU16) -> Self {
        Self {
            lines: Vec::new(),
            current: Vec::new(),
            gap: false,
            levels: Vec::new(),
            styles: Vec::new(),
            fill: None,
            width,
        }
    }

    /// Append `text` with the current decoration.
    ///
    /// Newlines end the current logical line, so code blocks and `pre`
    /// elements keep their line breaks.
    fn text(&mut self, text: &str) {
        let style = self.styles.last().copied().unwrap_or_default();
        self.append(&crate::ansi::strip(text), style);
    }

    /// Append `text` with the given decoration.
    fn styled_text(&mut self, text: &str, style: Style) {
        self.append(&crate::ansi::strip(text), style);
    }

    /// Append cleaned text, splitting at newlines.
    fn append(&mut self, cleaned: &str, style: Style) {
        for (index, part) in cleaned.split('\n').enumerate() {
            if index > 0 {
                self.end_line();
            }
            if part.is_empty() {
                continue;
            }
            match self.current.last_mut() {
                Some(last) if last.style == style => last.text.push_str(part),
                _ => self.current.push(StyledSpan {
                    text: part.to_owned(),
                    style,
                }),
            }
        }
    }

    /// Layer a decoration derived from the current one.
    fn push_style(&mut self, f: impl FnOnce(&mut Style)) {
        let mut style = self.styles.last().copied().unwrap_or_default();
        f(&mut style);
        self.styles.push(style);
    }

    /// Remove the most recent decoration layer.
    fn pop_style(&mut self) {
        self.styles.pop();
    }

    /// End the current logical line.
    fn end_line(&mut self) {
        if self.gap {
            self.gap = false;
            if !self.lines.is_empty() {
                self.push_empty();
            }
        }
        if self.current.is_empty() {
            if self.fill.is_some() {
                let (lead, hang) = self.body_indent();
                self.lines.push(Line {
                    lead,
                    hang,
                    spans: Vec::new(),
                    fill: self.fill,
                });
                self.mark_lead_used();
            } else {
                self.push_empty();
            }
            return;
        }
        let body = std::mem::take(&mut self.current);
        let (lead, hang) = self.body_indent();
        self.lines.push(Line {
            lead,
            hang,
            spans: body,
            fill: self.fill,
        });
        self.mark_lead_used();
    }

    /// End the current line when it holds text.
    fn flush(&mut self) {
        if !self.current.is_empty() {
            self.end_line();
        }
    }

    /// Ask for one blank line before the next content.
    fn gap(&mut self) {
        self.flush();
        self.gap = true;
    }

    /// Drop a pending blank line request.
    fn no_gap(&mut self) {
        self.flush();
        self.gap = false;
    }

    /// Enter an item with `marker` as its bullet or number.
    fn push_item(&mut self, marker: Vec<StyledSpan>) {
        let width: usize = marker
            .iter()
            .map(|span| UnicodeWidthStr::width(span.text.as_str()))
            .sum();
        let blank = " ".repeat(width);
        self.levels.push(Level {
            lead: marker,
            hang: vec![StyledSpan {
                text: blank,
                style: Style::default(),
            }],
            lead_used: false,
            item: true,
        });
    }

    /// Enter a quote level.
    fn push_quote(&mut self) {
        let marker = StyledSpan {
            text: "╏ ".to_owned(),
            style: Style {
                dim: true,
                ..Style::default()
            },
        };
        self.levels.push(Level {
            lead: vec![marker.clone()],
            hang: vec![marker],
            lead_used: false,
            item: false,
        });
    }

    /// Leave the innermost item or quote level.
    fn pop_level(&mut self) {
        self.flush();
        self.levels.pop();
    }

    /// One plus the number of enclosing item levels.
    fn depth(&self) -> usize {
        self.levels.iter().filter(|level| level.item).count() + 1
    }

    /// Width left for content after the current indentation.
    fn inner_width(&self) -> NonZeroU16 {
        let used: usize = self
            .levels
            .iter()
            .flat_map(|level| &level.hang)
            .map(|span| UnicodeWidthStr::width(span.text.as_str()))
            .sum();
        let width = usize::from(self.width.get());
        let rest = width.saturating_sub(used).max(1).min(usize::from(u16::MAX));
        NonZeroU16::new(rest as u16).expect("at least one column")
    }

    /// Set the background for code lines.
    fn set_fill(&mut self, fill: Option<Color>) {
        self.fill = fill;
    }

    /// Finish the assembly and return the logical lines.
    fn finish(mut self) -> Vec<Line> {
        self.flush();
        while self.lines.first().is_some_and(is_empty) {
            self.lines.remove(0);
        }
        while self.lines.last().is_some_and(is_empty) {
            self.lines.pop();
        }
        self.lines
    }

    /// Indentation of a body line.
    fn body_indent(&self) -> (Vec<StyledSpan>, Vec<StyledSpan>) {
        let mut lead = Vec::new();
        let mut hang = Vec::new();
        for level in &self.levels {
            if level.lead_used {
                extend_spans(&mut lead, &level.hang);
            } else {
                extend_spans(&mut lead, &level.lead);
            }
            extend_spans(&mut hang, &level.hang);
        }
        (lead, hang)
    }

    /// Mark every level as having emitted its first line.
    fn mark_lead_used(&mut self) {
        for level in &mut self.levels {
            level.lead_used = true;
        }
    }

    /// Push one blank line unless the last line is already blank.
    fn push_empty(&mut self) {
        if self.lines.last().is_some_and(is_empty) {
            return;
        }
        let mut joined: Vec<StyledSpan> = Vec::new();
        for level in &self.levels {
            extend_spans(&mut joined, &level.hang);
        }
        trim_trailing_blanks(&mut joined);
        self.lines.push(Line {
            lead: joined.clone(),
            hang: joined,
            spans: Vec::new(),
            fill: None,
        });
    }
}

/// Whether the line is a blank separator without a fill.
fn is_empty(line: &Line) -> bool {
    line.spans.is_empty() && line.fill.is_none()
}

/// Append `more` spans, joining neighbours with the same decoration.
fn extend_spans(into: &mut Vec<StyledSpan>, more: &[StyledSpan]) {
    for span in more {
        match into.last_mut() {
            Some(last) if last.style == span.style => last.text.push_str(&span.text),
            _ => into.push(span.clone()),
        }
    }
}

/// Remove trailing blanks from joined indentation.
fn trim_trailing_blanks(spans: &mut Vec<StyledSpan>) {
    while let Some(last) = spans.last_mut() {
        let trimmed = last.text.trim_end_matches(' ');
        if trimmed.is_empty() && last.text.chars().all(|c| c == ' ') {
            spans.pop();
        } else {
            last.text = trimmed.to_owned();
            break;
        }
    }
    spans.retain(|span| !span.text.is_empty());
}
