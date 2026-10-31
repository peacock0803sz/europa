//! Rendering of Markdown bodies as logical lines.
//!
//! CommonMark comes from pulldown-cmark events. Only the events a task
//! implements affect gaps and decorations and the rest keep their text.

use std::num::NonZeroU16;

use europa_core::{Background, Style, StyledSpan};
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::blocks::{Blocks, Line};

/// Open list with its numbering and nesting.
struct ListContext {
    ordered: bool,
    next: u64,
    nested: bool,
}

/// Render a Markdown body as logical lines.
pub(crate) fn render(text: &str, width: NonZeroU16, background: Background) -> Vec<Line> {
    let mut blocks = Blocks::new(width);
    let mut buffered = String::new();
    let mut lists: Vec<ListContext> = Vec::new();
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(text, options) {
        // Consecutive text is joined before control removal, because a
        // sequence can span the boundary pulldown-cmark reports.
        match &event {
            Event::Text(_) => {}
            _ => {
                if !buffered.is_empty() {
                    blocks.text(&buffered);
                    buffered.clear();
                }
            }
        }
        match event {
            Event::Start(Tag::Paragraph) => blocks.gap(),
            Event::End(TagEnd::Paragraph) => blocks.gap(),
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
            Event::Text(body) => buffered.push_str(&body),
            Event::Code(body) => {
                let bg = crate::code_bg(background);
                blocks.push_style(|style| style.bg = Some(bg));
                blocks.text(&body);
                blocks.pop_style();
            }
            Event::SoftBreak => {
                blocks.text(" ");
            }
            Event::HardBreak => {
                blocks.end_line();
            }
            _ => {}
        }
    }
    if !buffered.is_empty() {
        blocks.text(&buffered);
    }
    blocks.finish()
}
