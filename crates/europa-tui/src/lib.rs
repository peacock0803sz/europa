//! Terminal interface for Europa.
//!
//! The library draws what `europa-render` makes of a cell, including images
//! of PNG and JPEG outputs. [`run`] opens a notebook in the terminal, and
//! the probe program shows a notebook with images to check each protocol on
//! a real terminal.
//!
//! # How a viewer uses the drawing
//!
//! The viewer in `app.rs` opens a notebook and draws it, and the probe in
//! `probe.rs` is an example of each step.
//!
//! 1. `ImageRenderer::new` once, after `ratatui::init` and before the first
//!    event is read. It chooses the protocol and finds the size of a cell.
//! 2. `ImageRenderer::cell_pixels` goes into the `RenderOptions` that
//!    `render_outputs_with_images` gets, and `ImageRenderer::insert` receives
//!    the decoded images from it. That call decodes every image of the cell,
//!    so it is not made for every frame: keep the `RenderedCell`, and render
//!    again only when the outputs, the width, the line limit, the background
//!    or the size of a cell change.
//! 3. For every frame, `cell_view::visible_placements` for each visible cell,
//!    and one call of `ImageRenderer::placements_changed` with all of them.
//!    When it returns `true`, `clear_screen` is called before drawing.
//! 4. `cell_view::draw_cell` for each visible cell. The area is as wide as
//!    the width the cell was rendered for, and does not include the last line
//!    of the terminal, because a Sixel image drawn there makes the terminal
//!    scroll (an issue of `ratatui-image`).
//! 5. When writing the frame fails, `ImageRenderer::fall_back_visible` and
//!    `clear_screen`, and the frame is drawn again. The images that were on
//!    screen are then drawn with half-blocks.
//! 6. When the window is resized, `ImageRenderer::on_resize`. It returns
//!    `true` when the size of a cell changed, and the cells are rendered
//!    again then.

#![deny(missing_docs)]

mod app;
mod cell_display;
mod cell_view;
mod cli;
mod exit;
mod image_cache;
mod image_protocol;
mod image_view;
mod keymap;
mod probe;
#[cfg(test)]
mod tests;
mod viewer;
mod viewer_view;

use std::ffi::OsString;
use std::process::ExitCode;

/// Runs the program that shows a notebook with images, from its arguments
/// without the name of the program.
///
/// With a notebook, it is shown until `q` is pressed, and `j` and `k` scroll.
/// `--protocol` names a protocol instead of detecting one, `--panic` panics
/// after the first frame, `--write-sample` writes a notebook to look at,
/// `--bench` measures how long each protocol takes, and `--select-only`
/// writes the protocol that was chosen and how long that took. Wrong arguments
/// print how to use it and give the exit code 2.
pub fn run_image_probe(args: impl Iterator<Item = OsString>) -> ExitCode {
    match probe::parse_args(args) {
        Ok(command) => probe::run(command),
        Err(message) => {
            eprintln!("{message}\n{}", probe::USAGE);
            ExitCode::from(2)
        }
    }
}

/// Open a notebook in the terminal.
///
/// `args` holds the arguments without the name of the program. The exit
/// codes are 0 for success and `--help`, 64 for usage errors, 65 for a
/// notebook that cannot be parsed, 66 for a file that cannot be read, 74 for
/// a terminal that cannot be used, 129 for SIGHUP and 143 for SIGTERM.
pub fn run(args: impl Iterator<Item = OsString>) -> ExitCode {
    crate::app::run(args)
}
