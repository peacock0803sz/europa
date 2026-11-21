//! Choosing how images are drawn: the protocol, the size of a cell and the
//! number of colors.

use std::num::NonZeroU16;

use europa_core::{Background, CellPixels, ImageProtocolChoice};
use ratatui_image::picker::{Capability, ProtocolType};

/// How many colors the terminal shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorDepth {
    /// 24-bit colors.
    TrueColor,
    /// The 256-color palette.
    Indexed256,
}

/// What the terminal answered to the query.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Probe {
    /// The protocol the terminal was found to support.
    pub(crate) protocol: ProtocolType,
    /// The size of a cell the terminal reported, if it did.
    pub(crate) cell_pixels: Option<CellPixels>,
    /// The background color the terminal reported, if it did.
    pub(crate) background: Option<(u8, u8, u8)>,
}

/// Decide whether the background is dark or light.
///
/// Returns dark when there was no answer.
pub(crate) fn background_from(answer: Option<(u8, u8, u8)>) -> Background {
    match answer {
        Some((r, g, b)) => {
            let bright = u32::from(r) * 2126 + u32::from(g) * 7152 + u32::from(b) * 722;
            if bright > 1_275_000 {
                Background::Light
            } else {
                Background::Dark
            }
        }
        None => Background::Dark,
    }
}

/// What to find out before a protocol is chosen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Selection {
    /// Half-blocks, without a query to the terminal.
    Halfblock,
    /// Ask the terminal, and use `force` instead of what it supports when
    /// that is set.
    Query {
        /// A protocol that was asked for.
        force: Option<ProtocolType>,
    },
}

/// Decides whether the terminal is queried.
///
/// A choice of half-blocks and a session that is not interactive both skip
/// the query, because it would leave bytes in the input or in the output.
pub(crate) fn plan_selection(choice: ImageProtocolChoice, interactive: bool) -> Selection {
    if !interactive {
        return Selection::Halfblock;
    }
    let force = match choice {
        ImageProtocolChoice::Halfblock => return Selection::Halfblock,
        ImageProtocolChoice::Auto => None,
        ImageProtocolChoice::Kitty => Some(ProtocolType::Kitty),
        ImageProtocolChoice::Sixel => Some(ProtocolType::Sixel),
        ImageProtocolChoice::Iterm2 => Some(ProtocolType::Iterm2),
    };
    Selection::Query { force }
}

impl Selection {
    /// The protocol and the size of a cell to use.
    ///
    /// `probe` is what the query found, and is `None` only when the query
    /// failed to read or write. A terminal that does not answer is not a
    /// failure: it gives a probe with what the environment suggests.
    /// `window` is the size of a cell worked out from the size of the window.
    pub(crate) fn resolve(
        self,
        probe: Option<Probe>,
        window: Option<CellPixels>,
    ) -> (ProtocolType, CellPixels) {
        let (probe, protocol) = match self {
            // The query is not looked at at all, so that a choice of half-blocks
            // gives the same sizes whatever the terminal could have answered.
            Selection::Halfblock => (None, ProtocolType::Halfblocks),
            Selection::Query { force } => {
                let found = probe.map(|probe| probe.protocol);
                (probe, force.or(found).unwrap_or(ProtocolType::Halfblocks))
            }
        };
        let measured = probe.and_then(|probe| probe.cell_pixels).or(window);
        // A protocol that draws in pixels needs the real size of a cell, or
        // images spill out of their slots.
        let protocol = match measured {
            None => ProtocolType::Halfblocks,
            Some(_) => protocol,
        };
        (protocol, measured.unwrap_or(CellPixels::DEFAULT))
    }
}

/// Turns what the picker found into a [`Probe`].
pub(crate) fn probe_from(protocol: ProtocolType, capabilities: &[Capability]) -> Probe {
    let cell_pixels = capabilities.iter().find_map(|capability| match capability {
        Capability::CellSize(Some((width, height))) => Some(CellPixels {
            width: NonZeroU16::new(*width)?,
            height: NonZeroU16::new(*height)?,
        }),
        _ => None,
    });
    let background = capabilities.iter().find_map(|capability| match capability {
        Capability::Background(r, g, b) => Some((*r, *g, *b)),
        _ => None,
    });
    Probe {
        protocol,
        cell_pixels,
        background,
    }
}

/// How many colors the environment says the terminal shows.
pub(crate) fn color_depth(get: impl Fn(&str) -> Option<String>) -> ColorDepth {
    let direct = get("COLORTERM").is_some_and(|value| value == "truecolor" || value == "24bit")
        || get("TERM").is_some_and(|value| value.ends_with("-direct"));
    if direct {
        ColorDepth::TrueColor
    } else {
        ColorDepth::Indexed256
    }
}

/// The size of a cell from the size of the window in columns, lines and
/// pixels, or `None` when the window does not tell.
pub(crate) fn cell_pixels_from(
    columns: u16,
    rows: u16,
    width: u16,
    height: u16,
) -> Option<CellPixels> {
    if columns == 0 || rows == 0 {
        return None;
    }
    Some(CellPixels {
        width: NonZeroU16::new(width / columns)?,
        height: NonZeroU16::new(height / rows)?,
    })
}
