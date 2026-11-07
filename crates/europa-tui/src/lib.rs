//! Terminal interface for Europa.
//!
//! The library draws what `europa-render` makes of a cell, including images
//! of PNG and JPEG outputs. The only public item is the program that shows a
//! notebook with images, which is used to check each protocol on a real
//! terminal.

#![deny(missing_docs)]

mod cell_view;
mod image_cache;
mod image_protocol;
mod image_view;
mod probe;
#[cfg(test)]
mod tests;

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
