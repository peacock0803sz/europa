//! Choosing how images are drawn: the protocol, the size of a cell and the
//! number of colors.

use europa_core::{CellPixels, ImageProtocolChoice};
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
    todo!()
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
        todo!()
    }
}

/// Turns what the picker found into a [`Probe`].
pub(crate) fn probe_from(protocol: ProtocolType, capabilities: &[Capability]) -> Probe {
    todo!()
}

/// How many colors the environment says the terminal shows.
pub(crate) fn color_depth(get: impl Fn(&str) -> Option<String>) -> ColorDepth {
    todo!()
}

/// The size of a cell from the size of the window in columns, lines and
/// pixels, or `None` when the window does not tell.
pub(crate) fn cell_pixels_from(
    columns: u16,
    rows: u16,
    width: u16,
    height: u16,
) -> Option<CellPixels> {
    todo!()
}
