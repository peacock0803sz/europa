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
    let mut link_target: Option<String> = None;
    let mut link_text = String::new();
    let mut image_alt: Option<String> = None;
    let mut html_body: Option<String> = None;
    let mut html_tags: Vec<String> = Vec::new();
    let options = Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH;
    for event in Parser::new_ext(text, options) {
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
            Event::InlineHtml(body) => inline_html(&mut blocks, &mut html_tags, &body, background),
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
            _ => {}
        }
    }
    if !buffered.is_empty() {
        blocks.text(&buffered);
    }
    blocks.finish()
}

/// Interpret one inline HTML tag for emphasis and line breaks.
fn inline_html(blocks: &mut Blocks, stack: &mut Vec<String>, body: &str, background: Background) {
    let tag = tag_name(body);
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
