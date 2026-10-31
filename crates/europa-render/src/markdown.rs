//! Rendering of Markdown bodies as logical lines.
//!
//! CommonMark comes from pulldown-cmark events. Only the events a task
//! implements affect gaps and decorations and the rest keep their text.

use std::num::NonZeroU16;

use europa_core::Background;
use pulldown_cmark::{Event, Options, Parser, Tag, TagEnd};

use crate::blocks::{Blocks, Line};

/// Render a Markdown body as logical lines.
pub(crate) fn render(text: &str, width: NonZeroU16, _background: Background) -> Vec<Line> {
    let mut blocks = Blocks::new(width);
    let mut buffered = String::new();
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(text, options) {
        // Consecutive text is joined before control removal, because a
        // sequence can span the boundary pulldown-cmark reports.
        match &event {
            Event::Text(_) | Event::Code(_) => {}
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
            Event::Text(body) => buffered.push_str(&body),
            Event::Code(body) => buffered.push_str(&body),
            Event::SoftBreak => {
                if !buffered.is_empty() {
                    blocks.text(&buffered);
                    buffered.clear();
                }
                blocks.text(" ");
            }
            Event::HardBreak => {
                if !buffered.is_empty() {
                    blocks.text(&buffered);
                    buffered.clear();
                }
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
