//! Escape sequences and control characters in the text of an output.
//!
//! The text comes from a kernel or a notebook file, so it can hold any escape
//! sequence. Only SGR, which sets colors and attributes, is kept. Every other
//! escape sequence and control character is dropped, so that nothing in the
//! result can change the state of the terminal it is drawn on.

use std::iter::Peekable;
use std::str::Chars;

use europa_core::{Color, Style, StyledSpan};

type Input<'a> = Peekable<Chars<'a>>;

/// Split `body` at its newlines into logical lines, each as runs of one style.
///
/// The style starts plain and carries over from one line to the next. A line
/// that ends in a newline is returned even when it is empty. The text after
/// the last newline is returned only when a character is left in it, so a body
/// that ends in a newline or in an escape sequence adds no empty line.
pub(crate) fn parse(body: &str) -> Vec<Vec<StyledSpan>> {
    let mut lines = Vec::new();
    let mut runs: Vec<StyledSpan> = Vec::new();
    let mut style = Style::default();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\n' => lines.push(std::mem::take(&mut runs)),
            '\u{1b}' => escape(&mut chars, &mut style),
            // C1 controls such as U+009B are dropped like the other controls
            // instead of starting a sequence: UTF-8 terminals mostly ignore
            // them and kernels do not send them.
            c if c.is_control() && c != '\t' => {}
            c => push(&mut runs, c, style),
        }
    }
    if !runs.is_empty() {
        lines.push(runs);
    }
    lines
}

fn push(runs: &mut Vec<StyledSpan>, c: char, style: Style) {
    match runs.last_mut() {
        Some(last) if last.style == style => last.text.push(c),
        _ => runs.push(StyledSpan {
            text: c.to_string(),
            style,
        }),
    }
}

/// Read the rest of an escape sequence, after its ESC.
fn escape(chars: &mut Input<'_>, style: &mut Style) {
    match chars.peek() {
        Some('[') => {
            chars.next();
            csi(chars, style);
        }
        Some(']' | 'P' | 'X' | '^' | '_') => {
            chars.next();
            skip_string(chars);
        }
        // Any other sequence is intermediate characters and a final one.
        Some(' '..='~') => {
            while chars.next_if(|c| matches!(c, ' '..='/')).is_some() {}
            let _ = chars.next_if(|c| matches!(c, '0'..='~'));
        }
        // The ESC stands alone, and what follows is ordinary text.
        _ => {}
    }
}

/// Read a control sequence, after `ESC [`, and apply it if it is SGR.
fn csi(chars: &mut Input<'_>, style: &mut Style) {
    let mut params = String::new();
    let mut intermediate = false;
    while let Some(&c) = chars.peek() {
        match c {
            '0'..='?' => params.push(c),
            ' '..='/' => intermediate = true,
            '@'..='~' => {
                chars.next();
                // `<`, `=`, `>` and `?` mark a private sequence, not SGR.
                if c == 'm' && !intermediate && params.bytes().all(|byte| byte <= b';') {
                    sgr(&params, style);
                }
                return;
            }
            // Not part of a sequence, so the sequence ends here and the
            // character is read as the text or sequence it is.
            _ => return,
        }
        chars.next();
    }
}

/// Skip a string sequence, after `ESC ]` or the like, up to BEL or `ESC \`.
///
/// A terminal would go on past a newline, but a sequence that never ends
/// would then swallow the rest of the output, so a newline ends it, and so
/// does an ESC that does not start `ESC \`. A sequence that is well formed
/// holds neither, so it is read as a terminal reads it.
fn skip_string(chars: &mut Input<'_>) {
    while let Some(&c) = chars.peek() {
        match c {
            '\u{7}' => {
                chars.next();
                return;
            }
            '\n' => return,
            '\u{1b}' => {
                let mut ahead = chars.clone();
                ahead.next();
                if ahead.next() == Some('\\') {
                    *chars = ahead;
                }
                return;
            }
            _ => {
                chars.next();
            }
        }
    }
}

/// Apply the arguments of an SGR sequence, separated by `;`.
fn sgr(params: &str, style: &mut Style) {
    // An empty argument is 0. One that holds ':' or is too large for a byte
    // does not parse, and only that argument is skipped.
    let mut args = params.split(';').map(|arg| match arg {
        "" => Some(0),
        _ => arg.parse::<u8>().ok(),
    });
    while let Some(arg) = args.next() {
        match arg {
            Some(0) => *style = Style::default(),
            Some(1) => style.bold = true,
            Some(2) => style.dim = true,
            Some(3) => style.italic = true,
            Some(4) => style.underline = true,
            Some(7) => style.reverse = true,
            Some(9) => style.strikethrough = true,
            Some(22) => {
                style.bold = false;
                style.dim = false;
            }
            Some(23) => style.italic = false,
            Some(24) => style.underline = false,
            Some(27) => style.reverse = false,
            Some(29) => style.strikethrough = false,
            Some(n @ 30..=37) => style.fg = Some(Color::Indexed(n - 30)),
            Some(n @ 90..=97) => style.fg = Some(Color::Indexed(n - 90 + 8)),
            Some(n @ 40..=47) => style.bg = Some(Color::Indexed(n - 40)),
            Some(n @ 100..=107) => style.bg = Some(Color::Indexed(n - 100 + 8)),
            Some(38) => style.fg = extended(&mut args).or(style.fg),
            Some(48) => style.bg = extended(&mut args).or(style.bg),
            // The color of underlines has no place in a style, but its
            // arguments must be read so that they are not taken for numbers.
            Some(58) => {
                let _ = extended(&mut args);
            }
            Some(39) => style.fg = None,
            Some(49) => style.bg = None,
            _ => {}
        }
    }
}

/// Read the color after 38, 48 or 58: `5;n` or `2;r;g;b`.
///
/// A color that is incomplete or does not fit in a byte is `None`. If what
/// follows is neither form, the rest of the arguments cannot be told from
/// ordinary numbers, so they are skipped.
fn extended(args: &mut impl Iterator<Item = Option<u8>>) -> Option<Color> {
    match args.next()? {
        Some(5) => args.next()?.map(Color::Indexed),
        Some(2) => {
            let (r, g, b) = (args.next()?, args.next()?, args.next()?);
            Some(Color::Rgb(r?, g?, b?))
        }
        _ => {
            args.for_each(drop);
            None
        }
    }
}
