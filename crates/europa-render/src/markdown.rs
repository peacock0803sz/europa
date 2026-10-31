//! Rendering of Markdown bodies as logical lines.
//!
//! CommonMark comes from pulldown-cmark events. Events without a rule here
//! keep their text and add no gaps or decorations.

use std::num::NonZeroU16;

use europa_core::{Background, Style, StyledSpan};
use pulldown_cmark::{Alignment, Event, Options, Parser, Tag, TagEnd};
use unicode_width::UnicodeWidthStr;

use crate::blocks::{Blocks, Line};

/// Open list with its numbering and nesting.
struct ListContext {
    ordered: bool,
    next: u64,
    nested: bool,
}

/// Collecting table with its alignments and styled cells.
struct TableState {
    aligns: Vec<Alignment>,
    header: Vec<Vec<StyledSpan>>,
    rows: Vec<Vec<Vec<StyledSpan>>>,
    current_row: Vec<Vec<StyledSpan>>,
    current_cell: Vec<StyledSpan>,
    /// Consecutive text of the current cell, joined before control removal.
    cell_text: String,
    /// Target and text of the open link, for the target shown after it.
    link: Option<(String, String)>,
    cell_styles: Vec<Style>,
    in_head: bool,
}

/// Render a Markdown body as logical lines.
pub(crate) fn render(text: &str, width: NonZeroU16, background: Background) -> Vec<Line> {
    let mut blocks = Blocks::new(width);
    let mut buffered = String::new();
    let mut lists: Vec<ListContext> = Vec::new();
    let mut link_target: Option<String> = None;
    let mut link_text = String::new();
    let mut image_alt: Option<String> = None;
    let mut html_body: Option<String> = None;
    let mut html_tags: Vec<String> = Vec::new();
    let mut html_hidden: Option<String> = None;
    let mut table: Option<TableState> = None;
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(text, options) {
        if table.is_some() {
            handle_table_event(
                event,
                &mut table,
                &mut blocks,
                &mut image_alt,
                background,
                width,
            );
            continue;
        }
        if html_hidden.is_some()
            && matches!(
                event,
                Event::Text(_) | Event::Code(_) | Event::SoftBreak | Event::HardBreak
            )
        {
            continue;
        }
        // Consecutive text is joined before control removal, because a
        // sequence can span the boundary pulldown-cmark reports. Text inside
        // links is also collected for the target and the notice.
        match &event {
            Event::Text(body) => {
                if let Some(alt) = image_alt.as_mut() {
                    alt.push_str(body);
                } else {
                    buffered.push_str(body);
                    if link_target.is_some() {
                        link_text.push_str(body);
                    }
                }
            }
            _ => {
                if !buffered.is_empty() {
                    blocks.text(&buffered);
                    buffered.clear();
                }
            }
        }
        match event {
            Event::Start(Tag::Paragraph) => blocks.gap(),
            // Inline HTML ends with its block, even when it never closes.
            Event::End(TagEnd::Paragraph) => {
                html_hidden = None;
                blocks.gap();
            }
            Event::Start(Tag::Heading { level, .. }) => {
                blocks.gap();
                blocks.push_style(|style| match level {
                    pulldown_cmark::HeadingLevel::H1 => {
                        style.bold = true;
                        style.underline = true;
                    }
                    pulldown_cmark::HeadingLevel::H2 => {
                        style.bold = true;
                    }
                    _ => {
                        style.bold = true;
                        style.dim = true;
                    }
                });
            }
            Event::End(TagEnd::Heading(..)) => {
                html_hidden = None;
                blocks.pop_style();
                blocks.gap();
            }
            Event::Start(Tag::Emphasis) => {
                blocks.push_style(|style| style.italic = true);
            }
            Event::End(TagEnd::Emphasis) => blocks.pop_style(),
            Event::Start(Tag::Strong) => {
                blocks.push_style(|style| style.bold = true);
            }
            Event::End(TagEnd::Strong) => blocks.pop_style(),
            Event::Start(Tag::Strikethrough) => {
                blocks.push_style(|style| style.strikethrough = true);
            }
            Event::End(TagEnd::Strikethrough) => blocks.pop_style(),
            Event::Start(Tag::CodeBlock(_)) => {
                blocks.gap();
                blocks.set_fill(Some(crate::code_bg(background)));
            }
            Event::End(TagEnd::CodeBlock) => {
                blocks.flush();
                blocks.set_fill(None);
                blocks.gap();
            }
            Event::Start(Tag::List(start)) => {
                let nested = blocks.depth() >= 2;
                if nested {
                    blocks.no_gap();
                } else {
                    blocks.gap();
                }
                lists.push(ListContext {
                    ordered: start.is_some(),
                    next: start.unwrap_or(1),
                    nested,
                });
            }
            Event::End(TagEnd::List(_)) => {
                if let Some(context) = lists.pop() {
                    if context.nested {
                        blocks.no_gap();
                    } else {
                        blocks.gap();
                    }
                } else {
                    blocks.gap();
                }
            }
            Event::Start(Tag::Item) => {
                blocks.flush();
                let marker = match lists.last_mut() {
                    Some(context) if context.ordered => {
                        let number = context.next;
                        context.next += 1;
                        format!("{number}. ")
                    }
                    _ => match blocks.depth() {
                        1 => "\u{2023} ".to_owned(),
                        2 => "\u{25aa} ".to_owned(),
                        _ => "\u{25b8} ".to_owned(),
                    },
                };
                blocks.push_item(vec![StyledSpan {
                    text: marker,
                    style: Style::default(),
                }]);
            }
            Event::End(TagEnd::Item) => blocks.pop_level(),
            Event::Start(Tag::BlockQuote(_)) => {
                blocks.gap();
                blocks.push_quote();
            }
            Event::End(TagEnd::BlockQuote(_)) => {
                blocks.pop_level();
                blocks.gap();
            }
            Event::Rule => {
                blocks.gap();
                let rule = "\u{254c}".repeat(usize::from(blocks.inner_width().get()));
                blocks.styled_text(
                    &rule,
                    Style {
                        dim: true,
                        ..Style::default()
                    },
                );
                blocks.end_line();
                blocks.gap();
            }
            Event::Start(Tag::Link { dest_url, .. }) => {
                blocks.push_style(|style| style.underline = true);
                link_target = Some(dest_url.into_string());
                link_text.clear();
            }
            Event::End(TagEnd::Link) => {
                blocks.pop_style();
                if let Some(target) = link_target.take() {
                    if link_text != target {
                        blocks.styled_text(" ", Style::default());
                        blocks.styled_text(
                            &format!("({target})"),
                            Style {
                                dim: true,
                                ..Style::default()
                            },
                        );
                    }
                    link_text.clear();
                }
            }
            Event::Start(Tag::Image { .. }) => {
                image_alt = Some(String::new());
            }
            Event::End(TagEnd::Image) => {
                let alt = image_alt.take().unwrap_or_default();
                blocks.styled_text(
                    &format!("[image: {alt}]"),
                    Style {
                        dim: true,
                        ..Style::default()
                    },
                );
            }
            Event::Start(Tag::HtmlBlock) => {
                html_body = Some(String::new());
            }
            Event::Html(body) => {
                if let Some(collected) = html_body.as_mut() {
                    collected.push_str(&body);
                } else {
                    blocks.text(&body);
                }
            }
            Event::End(TagEnd::HtmlBlock) => {
                if let Some(collected) = html_body.take() {
                    blocks.gap();
                    let width = blocks.inner_width();
                    blocks.lines(crate::html::render(&collected, width, background));
                }
            }
            Event::InlineHtml(body) => inline_html(
                &mut blocks,
                &mut html_tags,
                &mut html_hidden,
                &body,
                background,
            ),
            Event::Text(_) => {}
            Event::Code(body) => {
                let bg = crate::code_bg(background);
                blocks.push_style(|style| style.bg = Some(bg));
                blocks.text(&body);
                blocks.pop_style();
                if link_target.is_some() {
                    link_text.push_str(&body);
                }
            }
            Event::SoftBreak => {
                blocks.text(" ");
                if link_target.is_some() {
                    link_text.push(' ');
                }
            }
            Event::HardBreak => {
                blocks.end_line();
            }
            Event::Start(Tag::Table(aligns)) => {
                blocks.gap();
                table = Some(TableState {
                    aligns,
                    header: Vec::new(),
                    rows: Vec::new(),
                    current_row: Vec::new(),
                    current_cell: Vec::new(),
                    cell_text: String::new(),
                    link: None,
                    cell_styles: Vec::new(),
                    in_head: false,
                });
            }
            _ => {}
        }
    }
    if !buffered.is_empty() {
        blocks.text(&buffered);
    }
    blocks.finish()
}

/// Handle one event inside a table.
///
/// Table cells collect styled spans instead of logical lines. The end of the
/// table builds aligned lines and merges them into the surrounding blocks.
fn handle_table_event(
    event: Event,
    table: &mut Option<TableState>,
    blocks: &mut Blocks,
    image_alt: &mut Option<String>,
    background: Background,
    _width: NonZeroU16,
) {
    let Some(state) = table.as_mut() else {
        return;
    };
    if !matches!(event, Event::Text(_) | Event::SoftBreak) {
        flush_cell_text(state);
    }
    match event {
        Event::Start(Tag::TableHead) => {
            state.in_head = true;
            state.current_row.clear();
        }
        Event::End(TagEnd::TableHead) => {
            state.header = std::mem::take(&mut state.current_row);
            state.in_head = false;
        }
        Event::Start(Tag::TableRow) => {
            state.in_head = false;
            state.current_row.clear();
        }
        Event::End(TagEnd::TableRow) => {
            let row = std::mem::take(&mut state.current_row);
            state.rows.push(row);
        }
        Event::Start(Tag::TableCell) => {
            state.current_cell.clear();
        }
        Event::End(TagEnd::TableCell) => {
            let cell = std::mem::take(&mut state.current_cell);
            if state.in_head
                || state.header.is_empty()
                || state.current_row.len() < state.header.len()
            {
                state.current_row.push(cell);
            }
        }
        Event::Text(body) => {
            if let Some(alt) = image_alt.as_mut() {
                alt.push_str(&body);
            } else {
                state.cell_text.push_str(&body);
                if let Some((_, text)) = state.link.as_mut() {
                    text.push_str(&body);
                }
            }
        }
        Event::Code(body) => {
            if let Some((_, text)) = state.link.as_mut() {
                text.push_str(&body);
            }
            let bg = crate::code_bg(background);
            push_cell_span(
                state,
                StyledSpan {
                    text: crate::ansi::strip(&body),
                    style: Style {
                        bg: Some(bg),
                        ..cell_style(state)
                    },
                },
            );
        }
        Event::SoftBreak => {
            state.cell_text.push(' ');
            if let Some((_, text)) = state.link.as_mut() {
                text.push(' ');
            }
        }
        Event::Start(Tag::Emphasis) => push_cell_style(state, |style| style.italic = true),
        Event::End(TagEnd::Emphasis) => {
            state.cell_styles.pop();
        }
        Event::Start(Tag::Strong) => push_cell_style(state, |style| style.bold = true),
        Event::End(TagEnd::Strong) => {
            state.cell_styles.pop();
        }
        Event::Start(Tag::Strikethrough) => {
            push_cell_style(state, |style| style.strikethrough = true);
        }
        Event::End(TagEnd::Strikethrough) => {
            state.cell_styles.pop();
        }
        Event::Start(Tag::Link { dest_url, .. }) => {
            push_cell_style(state, |style| style.underline = true);
            state.link = Some((dest_url.into_string(), String::new()));
        }
        Event::End(TagEnd::Link) => {
            state.cell_styles.pop();
            if let Some((target, text)) = state.link.take() {
                if text != target {
                    push_cell_span(
                        state,
                        StyledSpan {
                            text: " ".to_owned(),
                            style: Style::default(),
                        },
                    );
                    push_cell_span(
                        state,
                        StyledSpan {
                            text: crate::ansi::strip(&format!("({target})")),
                            style: Style {
                                dim: true,
                                ..Style::default()
                            },
                        },
                    );
                }
            }
        }
        Event::Start(Tag::Image { .. }) => {
            *image_alt = Some(String::new());
        }
        Event::End(TagEnd::Image) => {
            let alt = image_alt.take().unwrap_or_default();
            push_cell_span(
                state,
                StyledSpan {
                    text: format!("[image: {alt}]"),
                    style: Style {
                        dim: true,
                        ..Style::default()
                    },
                },
            );
        }
        Event::InlineHtml(body) => {
            let tag = tag_name(&body);
            match tag.as_str() {
                // A cell holds one display line, so a break only separates
                // the words around it.
                "br" => state.cell_text.push(' '),
                "b" | "strong" => {
                    if body.starts_with("</") {
                        state.cell_styles.pop();
                    } else if !body.ends_with("/>") {
                        push_cell_style(state, |style| style.bold = true);
                    }
                }
                "i" | "em" => {
                    if body.starts_with("</") {
                        state.cell_styles.pop();
                    } else if !body.ends_with("/>") {
                        push_cell_style(state, |style| style.italic = true);
                    }
                }
                "code" => {
                    if body.starts_with("</") {
                        state.cell_styles.pop();
                    } else if !body.ends_with("/>") {
                        let bg = crate::code_bg(background);
                        push_cell_style(state, |style| style.bg = Some(bg));
                    }
                }
                _ => {}
            }
        }
        Event::End(TagEnd::Table) => {
            let state = table.take().expect("table");
            let width = blocks.inner_width();
            blocks.lines(build_table(state, width));
            blocks.gap();
        }
        Event::Start(Tag::Table(_)) => {}
        _ => {}
    }
}

/// Current cell style, defaulting to plain text.
fn cell_style(state: &TableState) -> Style {
    state.cell_styles.last().copied().unwrap_or_default()
}

/// Layer a decoration for the rest of the current table cell.
fn push_cell_style(state: &mut TableState, change: impl FnOnce(&mut Style)) {
    let mut style = cell_style(state);
    change(&mut style);
    state.cell_styles.push(style);
}

/// Append the collected text to the current table cell with its decoration,
/// without escape sequences and control characters.
fn flush_cell_text(state: &mut TableState) {
    if state.cell_text.is_empty() {
        return;
    }
    let style = cell_style(state);
    let text = crate::ansi::strip(&std::mem::take(&mut state.cell_text));
    if text.is_empty() {
        return;
    }
    match state.current_cell.last_mut() {
        Some(last) if last.style == style => last.text.push_str(&text),
        _ => state.current_cell.push(StyledSpan { text, style }),
    }
}

/// Append one span to the current table cell, joining equal decorations.
fn push_cell_span(state: &mut TableState, span: StyledSpan) {
    if span.text.is_empty() {
        return;
    }
    match state.current_cell.last_mut() {
        Some(last) if last.style == span.style => last.text.push_str(&span.text),
        _ => state.current_cell.push(span),
    }
}

/// Build aligned logical lines from collected table cells.
fn build_table(state: TableState, width: NonZeroU16) -> Vec<Line> {
    let columns = state.header.len();
    if columns == 0 {
        return Vec::new();
    }
    let mut rows: Vec<Vec<Vec<StyledSpan>>> = Vec::new();
    rows.push(state.header);
    for mut row in state.rows {
        row.truncate(columns);
        while row.len() < columns {
            row.push(Vec::new());
        }
        rows.push(row);
    }
    let mut widths = vec![0usize; columns];
    for row in &rows {
        for (index, cell) in row.iter().enumerate() {
            let used: usize = cell
                .iter()
                .map(|span| UnicodeWidthStr::width(span.text.as_str()))
                .sum();
            widths[index] = widths[index].max(used);
        }
    }
    let total: usize = widths.iter().sum::<usize>() + 3 * columns.saturating_sub(1);
    let inner = usize::from(width.get());
    if total > inner {
        return rows
            .into_iter()
            .enumerate()
            .map(|(number, cells)| {
                let mut line = join_cells(cells);
                if number == 0 {
                    for span in &mut line.spans {
                        span.style.bold = true;
                    }
                }
                line
            })
            .collect();
    }
    let mut lines = Vec::new();
    for (number, row) in rows.into_iter().enumerate() {
        let header = number == 0;
        lines.push(padded_row(row, &state.aligns, &widths, header));
        if header {
            lines.push(separator_row(total));
        }
    }
    lines
}

/// Join one table row with separators and no padding.
fn join_cells(cells: Vec<Vec<StyledSpan>>) -> Line {
    let mut spans = Vec::new();
    for (index, cell) in cells.into_iter().enumerate() {
        if index > 0 {
            spans.push(StyledSpan {
                text: " \u{254e} ".to_owned(),
                style: Style::default(),
            });
        }
        spans.extend(cell);
    }
    Line {
        lead: Vec::new(),
        hang: Vec::new(),
        spans,
        fill: None,
    }
}

/// Pad one table row to its column widths.
fn padded_row(
    cells: Vec<Vec<StyledSpan>>,
    aligns: &[Alignment],
    widths: &[usize],
    header: bool,
) -> Line {
    let mut spans = Vec::new();
    for (index, cell) in cells.into_iter().enumerate() {
        if index > 0 {
            spans.push(StyledSpan {
                text: " \u{254e} ".to_owned(),
                style: Style::default(),
            });
        }
        let used: usize = cell
            .iter()
            .map(|span| UnicodeWidthStr::width(span.text.as_str()))
            .sum();
        let padded = pad_cell(
            cell,
            widths[index].saturating_sub(used),
            aligns.get(index),
            header,
        );
        spans.extend(padded);
    }
    if header {
        for span in &mut spans {
            span.style.bold = true;
        }
    }
    Line {
        lead: Vec::new(),
        hang: Vec::new(),
        spans,
        fill: None,
    }
}

/// Pad one cell to its column width without styling the blanks.
fn pad_cell(
    cell: Vec<StyledSpan>,
    missing: usize,
    align: Option<&Alignment>,
    header: bool,
) -> Vec<StyledSpan> {
    if missing == 0 {
        return cell;
    }
    let style = if header {
        Style {
            bold: true,
            ..Style::default()
        }
    } else {
        Style::default()
    };
    let blank = |count: usize| StyledSpan {
        text: " ".repeat(count),
        style,
    };
    let mut padded = Vec::new();
    match align {
        Some(Alignment::Right) => {
            padded.push(blank(missing));
            padded.extend(cell);
        }
        Some(Alignment::Center) => {
            let left = missing / 2;
            padded.push(blank(left));
            padded.extend(cell);
            padded.push(blank(missing - left));
        }
        _ => {
            padded.extend(cell);
            padded.push(blank(missing));
        }
    }
    padded
}

/// Thin rule under the header row.
fn separator_row(total: usize) -> Line {
    Line {
        lead: Vec::new(),
        hang: Vec::new(),
        spans: vec![StyledSpan {
            text: "\u{254c}".repeat(total),
            style: Style {
                dim: true,
                ..Style::default()
            },
        }],
        fill: None,
    }
}

/// Interpret one inline HTML tag for emphasis, line breaks and replaced
/// elements.
///
/// `hidden` names the open element whose content is left out, such as a
/// `script` or an `svg`, until its end tag arrives.
fn inline_html(
    blocks: &mut Blocks,
    stack: &mut Vec<String>,
    hidden: &mut Option<String>,
    body: &str,
    background: Background,
) {
    let tag = tag_name(body);
    let closing = body.starts_with("</");
    if let Some(name) = hidden.as_ref() {
        if closing && *name == tag {
            *hidden = None;
        }
        return;
    }
    if !closing {
        if (tag == "script" || tag == "style") && !body.ends_with("/>") {
            *hidden = Some(tag);
            return;
        }
        if let Some((notice, holds)) = crate::html::replaced_start(body) {
            blocks.styled_text(
                &notice,
                Style {
                    dim: true,
                    ..Style::default()
                },
            );
            if holds {
                *hidden = Some(tag);
            }
            return;
        }
    }
    match tag.as_str() {
        "br" => {
            blocks.end_line();
        }
        "b" | "strong" => {
            if body.starts_with("</") {
                if stack.last().is_some_and(|open| open == &tag) {
                    stack.pop();
                    blocks.pop_style();
                }
            } else if !body.ends_with("/>") {
                stack.push(tag);
                blocks.push_style(|style| style.bold = true);
            }
        }
        "i" | "em" => {
            if body.starts_with("</") {
                if stack.last().is_some_and(|open| open == &tag) {
                    stack.pop();
                    blocks.pop_style();
                }
            } else if !body.ends_with("/>") {
                stack.push(tag);
                blocks.push_style(|style| style.italic = true);
            }
        }
        "code" => {
            if body.starts_with("</") {
                if stack.last().is_some_and(|open| open == &tag) {
                    stack.pop();
                    blocks.pop_style();
                }
            } else if !body.ends_with("/>") {
                stack.push(tag);
                let bg = crate::code_bg(background);
                blocks.push_style(|style| style.bg = Some(bg));
            }
        }
        _ => {}
    }
}

/// Lowercased tag name of `<b>`, `</b>` or `<br/>`.
fn tag_name(body: &str) -> String {
    let mut name = String::new();
    let mut chars = body.chars();
    let _ = chars.next();
    if chars.clone().next() == Some('/') {
        let _ = chars.next();
    }
    for c in chars {
        if c.is_ascii_alphanumeric() {
            name.push(c.to_ascii_lowercase());
        } else {
            break;
        }
    }
    name
}
