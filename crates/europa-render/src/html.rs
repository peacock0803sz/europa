//! Small tokenizer that strips HTML tags for terminal display.
//!
//! The tokenizer keeps text and the shape of block and inline elements. A
//! full tree is more than display needs, so broken input closes whatever is
//! open at the end of the body.

use std::num::NonZeroU16;

use europa_core::{Background, Style, StyledSpan};

use crate::ansi;
use crate::blocks::{Blocks, Line};

/// One piece of an HTML body.
enum Token {
    /// Characters with entities replaced and controls removed.
    Text(String),
    /// Opening tag with a lowercased name and the `alt` of an `img`.
    Start {
        name: String,
        alt: Option<String>,
        self_closing: bool,
    },
    /// Closing tag with a lowercased name.
    End(String),
}

/// Split a body into tokens, dropping comments and hidden elements.
///
/// Text tokens already went through entity replacement and control removal.
fn tokenize(text: &str) -> Vec<Token> {
    let chars: Vec<char> = text.chars().collect();
    let mut tokens = Vec::new();
    let mut body = String::new();
    let mut pos = 0;
    let flush = |body: &mut String, tokens: &mut Vec<Token>| {
        if body.is_empty() {
            return;
        }
        let taken = std::mem::take(body);
        let decoded = htmlize::unescape(taken.as_str());
        let kept = ansi::strip(decoded.as_ref());
        if !kept.is_empty() {
            tokens.push(Token::Text(kept));
        }
    };
    while pos < chars.len() {
        if chars[pos] != '<' {
            body.push(chars[pos]);
            pos += 1;
            continue;
        }
        let next = chars.get(pos + 1).copied();
        match next {
            Some('!') => {
                if chars.get(pos + 2) == Some(&'-') && chars.get(pos + 3) == Some(&'-') {
                    flush(&mut body, &mut tokens);
                    match find_comment_end(&chars, pos + 4) {
                        Some(end) => pos = end,
                        None => break,
                    }
                } else {
                    flush(&mut body, &mut tokens);
                    match find_decl_end(&chars, pos + 2) {
                        Some(end) => pos = end,
                        None => break,
                    }
                }
            }
            Some('?') => {
                flush(&mut body, &mut tokens);
                match find_decl_end(&chars, pos + 2) {
                    Some(end) => pos = end,
                    None => break,
                }
            }
            Some(c) if c.is_ascii_alphabetic() => match read_start(&chars, pos) {
                Some((name, alt, self_closing, end)) => {
                    flush(&mut body, &mut tokens);
                    pos = end;
                    if (name == "script" || name == "style") && !self_closing {
                        match skip_hidden(&chars, pos, &name) {
                            Some(rest) => pos = rest,
                            None => break,
                        }
                    } else {
                        tokens.push(Token::Start {
                            name,
                            alt,
                            self_closing,
                        });
                    }
                }
                None => break,
            },
            Some('/') => {
                let after = chars.get(pos + 2).copied();
                match after {
                    Some(c) if c.is_ascii_alphanumeric() => match read_end(&chars, pos) {
                        Some((name, end)) => {
                            flush(&mut body, &mut tokens);
                            pos = end;
                            tokens.push(Token::End(name));
                        }
                        None => break,
                    },
                    _ => {
                        body.push('<');
                        pos += 1;
                    }
                }
            }
            _ => {
                body.push('<');
                pos += 1;
            }
        }
    }
    flush(&mut body, &mut tokens);
    tokens
}

/// Index after `-->` starting the search at `from`, if it closes.
fn find_comment_end(chars: &[char], from: usize) -> Option<usize> {
    let mut pos = from;
    while pos + 2 < chars.len() + 1 {
        if chars.get(pos) == Some(&'-')
            && chars.get(pos + 1) == Some(&'-')
            && chars.get(pos + 2) == Some(&'>')
        {
            return Some(pos + 3);
        }
        pos += 1;
    }
    None
}

/// Index after the `>` that ends a declaration, respecting quotes.
fn find_decl_end(chars: &[char], from: usize) -> Option<usize> {
    let mut pos = from;
    let mut quote = None;
    while pos < chars.len() {
        let c = chars[pos];
        if let Some(q) = quote {
            if c == q {
                quote = None;
            }
        } else if c == '"' || c == '\'' {
            quote = Some(c);
        } else if c == '>' {
            return Some(pos + 1);
        }
        pos += 1;
    }
    None
}

/// Read `<name ...>` or `<name .../>` at `pos`.
///
/// Returns the lowercased name, the unescaped `alt` of an `img`, whether the
/// tag self-closes, and the index after `>`. A tag that never closes yields
/// `None` and is dropped.
fn read_start(chars: &[char], pos: usize) -> Option<(String, Option<String>, bool, usize)> {
    let mut end = pos + 1;
    while end < chars.len() && chars[end].is_ascii_alphanumeric() {
        end += 1;
    }
    let name: String = chars[pos + 1..end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    let mut alt = None;
    let mut idx = end;
    let mut self_closing = false;
    loop {
        while idx < chars.len() && chars[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= chars.len() {
            return None;
        }
        if chars[idx] == '>' {
            return Some((name, alt, self_closing, idx + 1));
        }
        if chars[idx] == '/' && chars.get(idx + 1) == Some(&'>') {
            self_closing = true;
            return Some((name, alt, self_closing, idx + 2));
        }
        if chars[idx] == '/' {
            idx += 1;
            continue;
        }
        let attr_start = idx;
        while idx < chars.len()
            && !chars[idx].is_ascii_whitespace()
            && !matches!(chars[idx], '=' | '>' | '/' | '"' | '\'')
        {
            idx += 1;
        }
        if idx == attr_start {
            idx += 1;
            continue;
        }
        let attr: String = chars[attr_start..idx]
            .iter()
            .collect::<String>()
            .to_ascii_lowercase();
        while idx < chars.len() && chars[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if chars.get(idx) != Some(&'=') {
            continue;
        }
        idx += 1;
        while idx < chars.len() && chars[idx].is_ascii_whitespace() {
            idx += 1;
        }
        if idx >= chars.len() {
            return None;
        }
        let value = if chars[idx] == '"' || chars[idx] == '\'' {
            let quote = chars[idx];
            idx += 1;
            let start = idx;
            while idx < chars.len() && chars[idx] != quote {
                idx += 1;
            }
            if idx >= chars.len() {
                return None;
            }
            let raw: String = chars[start..idx].iter().collect();
            idx += 1;
            raw
        } else {
            let start = idx;
            while idx < chars.len()
                && !chars[idx].is_ascii_whitespace()
                && !matches!(chars[idx], '>' | '"' | '\'' | '`')
            {
                if chars[idx] == '/' && chars.get(idx + 1) == Some(&'>') {
                    break;
                }
                idx += 1;
            }
            chars[start..idx].iter().collect()
        };
        if name == "img" && attr == "alt" && alt.is_none() {
            alt = Some(htmlize::unescape(value.as_str()).into_owned());
        }
    }
}

/// Read `</name>` at `pos`, returning the lowercased name and the index after
/// `>`. A tag that never closes yields `None` and is dropped.
fn read_end(chars: &[char], pos: usize) -> Option<(String, usize)> {
    let mut end = pos + 2;
    while end < chars.len() && chars[end].is_ascii_alphanumeric() {
        end += 1;
    }
    let name: String = chars[pos + 2..end]
        .iter()
        .collect::<String>()
        .to_ascii_lowercase();
    let mut idx = end;
    while idx < chars.len() && chars[idx].is_ascii_whitespace() {
        idx += 1;
    }
    if chars.get(idx) == Some(&'>') {
        Some((name, idx + 1))
    } else if chars.get(idx) == Some(&'/') && chars.get(idx + 1) == Some(&'>') {
        Some((name, idx + 2))
    } else {
        None
    }
}

/// Index after the closing tag of a hidden element starting the search at
/// `from`, or `None` when it never closes.
fn skip_hidden(chars: &[char], from: usize, name: &str) -> Option<usize> {
    let mut pos = from;
    while pos + 2 < chars.len() {
        if chars[pos] == '<'
            && chars[pos + 1] == '/'
            && chars[pos + 2..]
                .iter()
                .take(name.len())
                .collect::<String>()
                .eq_ignore_ascii_case(name)
        {
            let mut end = pos + 2 + name.len();
            while end < chars.len() && chars[end].is_ascii_whitespace() {
                end += 1;
            }
            if chars.get(end) == Some(&'>') {
                return Some(end + 1);
            }
            if chars.get(end) == Some(&'/') && chars.get(end + 1) == Some(&'>') {
                return Some(end + 2);
            }
        }
        pos += 1;
    }
    None
}

/// Frame of an open element that needs cleanup when it closes.
struct Frame {
    name: String,
    kind: Kind,
}

/// Cleanup behind one open element.
enum Kind {
    Style,
    Quote,
    Item,
    List {
        ordered: bool,
        next: usize,
        nested: bool,
    },
    Pre,
    Table,
    Row {
        has_cell: bool,
    },
    Cell {
        bold: bool,
    },
    Gap,
    Flush,
}

/// Render an HTML body as logical lines.
///
/// Blocks share their gaps so nested `div` elements add no blank lines.
/// Unclosed elements close at the end of the body.
pub(crate) fn render(text: &str, width: NonZeroU16, background: Background) -> Vec<Line> {
    let tokens = tokenize(text);
    let mut renderer = Renderer {
        blocks: Blocks::new(width),
        pending: false,
        has_content: false,
        pre: false,
        head_depth: 0,
        open: Vec::new(),
        background,
        jump_to: None,
    };
    let mut index = 0;
    while index < tokens.len() {
        match &tokens[index] {
            Token::Text(body) => {
                renderer.text(body.clone());
                index += 1;
            }
            Token::Start {
                name,
                alt,
                self_closing,
            } => {
                let skip = renderer.start(name.clone(), alt.clone(), *self_closing, &tokens, index);
                if skip == Skip::Jumped {
                    index = renderer.jump_to.take().expect("skip sets a target");
                    continue;
                }
                index += 1;
            }
            Token::End(name) => {
                renderer.end(name.clone());
                index += 1;
            }
        }
    }
    renderer.blocks.finish()
}

/// Whether `start` consumed following tokens while skipping hidden content.
#[derive(PartialEq)]
enum Skip {
    Kept,
    Jumped,
}

/// Assembly of logical lines with pending blanks and open elements.
struct Renderer {
    blocks: Blocks,
    pending: bool,
    has_content: bool,
    pre: bool,
    head_depth: usize,
    open: Vec<Frame>,
    background: Background,
    jump_to: Option<usize>,
}

impl Renderer {
    fn text(&mut self, body: String) {
        if self.head_depth > 0 {
            return;
        }
        if self.pre {
            if !body.is_empty() {
                self.blocks.text(&body);
                self.has_content = !body.ends_with('\n');
                self.pending = false;
            }
            return;
        }
        let mut word = String::new();
        let flush_word = |renderer: &mut Renderer, word: &mut String| {
            if word.is_empty() {
                return;
            }
            if renderer.pending && renderer.has_content {
                renderer.blocks.text(" ");
            }
            renderer.blocks.text(word);
            renderer.has_content = true;
            renderer.pending = false;
            word.clear();
        };
        for c in body.chars() {
            if c == ' ' || c == '\t' || c == '\n' {
                let mut taken = std::mem::take(&mut word);
                flush_word(self, &mut taken);
                word = taken;
                self.pending = true;
            } else {
                word.push(c);
            }
        }
        let mut taken = std::mem::take(&mut word);
        flush_word(self, &mut taken);
    }

    fn start(
        &mut self,
        name: String,
        alt: Option<String>,
        self_closing: bool,
        tokens: &[Token],
        index: usize,
    ) -> Skip {
        self.jump_to = None;
        if self.head_depth > 0 {
            if name == "head" && !self_closing {
                self.head_depth += 1;
            }
            return Skip::Kept;
        }
        if name == "head" {
            if !self_closing {
                self.head_depth = 1;
            }
            return Skip::Kept;
        }
        if name == "script" || name == "style" {
            return Skip::Kept;
        }
        if is_replaced(&name) {
            let notice = replaced_notice(&name, alt);
            self.emit_notice(notice);
            if name == "img" || name == "embed" || self_closing {
                return Skip::Kept;
            }
            match find_matching_end(tokens, index, &name) {
                Some(end) => {
                    self.jump_to = Some(end + 1);
                    return Skip::Jumped;
                }
                None => {
                    self.jump_to = Some(tokens.len());
                    return Skip::Jumped;
                }
            }
        }
        match name.as_str() {
            "p" => {
                self.do_gap();
                self.open.push(Frame {
                    name,
                    kind: Kind::Gap,
                });
            }
            "h1" => {
                self.do_gap();
                self.blocks.push_style(|s| {
                    s.bold = true;
                    s.underline = true;
                });
                self.open.push(Frame {
                    name: name.clone(),
                    kind: Kind::Gap,
                });
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            "h2" => {
                self.do_gap();
                self.blocks.push_style(|s| s.bold = true);
                self.open.push(Frame {
                    name: name.clone(),
                    kind: Kind::Gap,
                });
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            "h3" | "h4" | "h5" | "h6" => {
                self.do_gap();
                self.blocks.push_style(|s| {
                    s.bold = true;
                    s.dim = true;
                });
                self.open.push(Frame {
                    name: name.clone(),
                    kind: Kind::Gap,
                });
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            "ul" | "ol" => {
                let nested = self.blocks.depth() >= 2;
                if nested {
                    self.do_no_gap();
                } else {
                    self.do_gap();
                }
                let ordered = name == "ol";
                self.open.push(Frame {
                    name,
                    kind: Kind::List {
                        ordered,
                        next: 1,
                        nested,
                    },
                });
            }
            "li" => {
                self.do_flush();
                let marker = self.item_marker();
                self.blocks.push_item(marker);
                self.has_content = false;
                self.pending = false;
                self.open.push(Frame {
                    name,
                    kind: Kind::Item,
                });
            }
            "blockquote" => {
                self.do_gap();
                self.blocks.push_quote();
                self.has_content = false;
                self.pending = false;
                self.open.push(Frame {
                    name,
                    kind: Kind::Quote,
                });
            }
            "pre" => {
                self.do_gap();
                self.blocks.set_fill(Some(crate::code_bg(self.background)));
                self.pre = true;
                self.has_content = false;
                self.pending = false;
                self.open.push(Frame {
                    name,
                    kind: Kind::Pre,
                });
            }
            "table" => {
                self.do_gap();
                self.open.push(Frame {
                    name,
                    kind: Kind::Table,
                });
            }
            "tr" => {
                self.do_flush();
                self.open.push(Frame {
                    name,
                    kind: Kind::Row { has_cell: false },
                });
            }
            "td" | "th" => {
                let bold = name == "th";
                let mut separated = false;
                for frame in self.open.iter_mut().rev() {
                    if let Kind::Row { has_cell } = &mut frame.kind {
                        if *has_cell {
                            separated = true;
                        }
                        break;
                    }
                    if matches!(frame.kind, Kind::Table) {
                        break;
                    }
                }
                if separated {
                    self.pending = false;
                    self.blocks.styled_text(" \u{254e} ", Style::default());
                    self.has_content = true;
                }
                if bold {
                    self.blocks.push_style(|s| s.bold = true);
                }
                self.open.push(Frame {
                    name,
                    kind: Kind::Cell { bold },
                });
            }
            "div" => {
                self.do_flush();
                self.open.push(Frame {
                    name,
                    kind: Kind::Flush,
                });
            }
            "br" => {
                self.pending = false;
                self.blocks.end_line();
                self.has_content = false;
            }
            "hr" => {
                self.do_gap();
                let rule = "\u{254c}".repeat(usize::from(self.blocks.inner_width().get()));
                self.blocks.styled_text(
                    &rule,
                    Style {
                        dim: true,
                        ..Style::default()
                    },
                );
                self.has_content = true;
                self.pending = false;
                self.blocks.end_line();
                self.has_content = false;
                self.do_gap();
            }
            "b" | "strong" => {
                self.flush_pending();
                self.blocks.push_style(|s| s.bold = true);
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            "i" | "em" => {
                self.flush_pending();
                self.blocks.push_style(|s| s.italic = true);
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            "code" if !self.pre => {
                self.flush_pending();
                let bg = crate::code_bg(self.background);
                self.blocks.push_style(|s| s.bg = Some(bg));
                self.open.push(Frame {
                    name,
                    kind: Kind::Style,
                });
            }
            _ => {}
        }
        if self_closing {
            let closed = name_of_self_closing(tokens, index);
            self.end(closed);
        }
        Skip::Kept
    }

    fn end(&mut self, name: String) {
        if self.head_depth > 0 {
            if name == "head" {
                self.head_depth -= 1;
            }
            return;
        }
        if name == "hr" || name == "br" || name == "img" || name == "embed" {
            return;
        }
        if name == "a" {
            return;
        }
        let position = self.open.iter().rposition(|frame| frame.name == name);
        let Some(top) = position else {
            return;
        };
        match name.as_str() {
            "p" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "blockquote" | "pre" | "table" => {
                self.do_gap();
            }
            "ul" | "ol" => {
                let nested = self.open[top..].iter().any(|frame| match &frame.kind {
                    Kind::List { nested, .. } if frame.name == name => *nested,
                    _ => false,
                });
                if nested {
                    self.do_no_gap();
                } else {
                    self.do_gap();
                }
            }
            "div" | "tr" => {
                self.do_flush();
            }
            _ => {}
        }
        if name == "pre" {
            self.do_flush();
        }
        while self.open.len() > top {
            let frame = self.open.pop().expect("top exists");
            self.cleanup(frame);
        }
        if name == "pre" {
            self.do_gap();
        }
    }

    fn flush_pending(&mut self) {
        if self.pending && self.has_content {
            self.blocks.text(" ");
        }
        self.pending = false;
    }

    fn cleanup(&mut self, frame: Frame) {
        match frame.kind {
            Kind::Style => {
                self.flush_pending();
                self.blocks.pop_style();
            }
            Kind::Quote | Kind::Item => {
                self.blocks.pop_level();
                self.has_content = false;
                self.pending = false;
            }
            Kind::List { .. } => {}
            Kind::Pre => {
                self.blocks.set_fill(None);
                self.pre = false;
                self.has_content = false;
                self.pending = false;
            }
            Kind::Table => {}
            Kind::Row { .. } => {
                self.has_content = false;
                self.pending = false;
            }
            Kind::Cell { bold } => {
                if bold {
                    self.flush_pending();
                    self.blocks.pop_style();
                }
                for frame in self.open.iter_mut().rev() {
                    if let Kind::Row { has_cell } = &mut frame.kind {
                        *has_cell = true;
                        break;
                    }
                    if matches!(frame.kind, Kind::Table) {
                        break;
                    }
                }
            }
            Kind::Gap | Kind::Flush => {}
        }
    }

    fn do_gap(&mut self) {
        self.pending = false;
        self.blocks.gap();
        self.has_content = false;
    }

    fn do_no_gap(&mut self) {
        self.pending = false;
        self.blocks.no_gap();
        self.has_content = false;
    }

    fn do_flush(&mut self) {
        self.pending = false;
        self.blocks.flush();
        self.has_content = false;
    }

    fn item_marker(&mut self) -> Vec<StyledSpan> {
        let mut ordered = None;
        let mut number = 1;
        for frame in self.open.iter_mut().rev() {
            if let Kind::List {
                ordered: is_ordered,
                next,
                ..
            } = &mut frame.kind
            {
                ordered = Some(*is_ordered);
                number = *next;
                *next += 1;
                break;
            }
        }
        match ordered {
            Some(true) => vec![StyledSpan {
                text: format!("{number}. "),
                style: Style::default(),
            }],
            _ => {
                let depth = self.blocks.depth();
                let mark = match depth {
                    1 => "\u{2023} ",
                    2 => "\u{25aa} ",
                    _ => "\u{25b8} ",
                };
                vec![StyledSpan {
                    text: mark.to_owned(),
                    style: Style::default(),
                }]
            }
        }
    }

    fn emit_notice(&mut self, notice: String) {
        if self.pending && self.has_content {
            self.blocks.text(" ");
        }
        self.blocks.styled_text(
            &notice,
            Style {
                dim: true,
                ..Style::default()
            },
        );
        self.has_content = true;
        self.pending = false;
    }
}

/// Elements replaced by a notice instead of their content.
fn is_replaced(name: &str) -> bool {
    matches!(
        name,
        "img" | "svg" | "iframe" | "video" | "audio" | "canvas" | "object" | "embed"
    )
}

/// Notice text for a replaced element.
fn replaced_notice(name: &str, alt: Option<String>) -> String {
    if name == "img" {
        match alt {
            Some(text) if !text.is_empty() => format!("[img: {text}]"),
            _ => "[img]".to_owned(),
        }
    } else {
        format!("[{name}]")
    }
}

/// Index of the end tag closing the replaced element opened at `open`.
///
/// Nested elements with the same name count so the matching end is found.
/// `None` means the element never closes.
fn find_matching_end(tokens: &[Token], open: usize, name: &str) -> Option<usize> {
    let mut depth = 0;
    for (index, token) in tokens.iter().enumerate().skip(open + 1) {
        match token {
            Token::Start {
                name: next,
                self_closing: false,
                ..
            } if next == name => {
                depth += 1;
            }
            Token::End(next) if next == name => {
                if depth == 0 {
                    return Some(index);
                }
                depth -= 1;
            }
            _ => {}
        }
    }
    None
}

/// Name for the synthetic end of a self-closed element.
fn name_of_self_closing(tokens: &[Token], index: usize) -> String {
    match &tokens[index] {
        Token::Start { name, .. } => name.clone(),
        _ => String::new(),
    }
}
