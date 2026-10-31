//! Small tokenizer that strips HTML tags for terminal display.
//!
//! The tokenizer keeps text and the shape of block and inline elements. A
//! full tree is more than display needs, so broken input closes whatever is
//! open at the end of the body.

use crate::ansi;

/// One piece of an HTML body.
#[allow(dead_code)]
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
#[allow(dead_code)]
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
