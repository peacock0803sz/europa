//! The program that shows a notebook with images in a terminal, to check each
//! protocol by eye, and the measurements that go with it.

use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use europa_core::ImageProtocolChoice;
use ratatui_image::picker::ProtocolType;

/// What the program was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProbeCommand {
    /// Show a notebook.
    View {
        /// The notebook to show.
        notebook: PathBuf,
        /// How the protocol is chosen.
        choice: ImageProtocolChoice,
        /// Panic after the first frame, to see that the terminal is restored.
        panic: bool,
    },
    /// Write the notebook of the sample to a file.
    WriteSample(PathBuf),
    /// Measure how long each protocol takes for a large image.
    Bench,
    /// Choose a protocol, and write what was chosen and how long it took.
    SelectOnly(PathBuf),
}

/// Reads the arguments, without touching the terminal or any file.
pub(crate) fn parse_args(args: impl Iterator<Item = OsString>) -> Result<ProbeCommand, String> {
    todo!()
}

/// The JSON of a notebook with one code cell, whose outputs show what the
/// drawing has to get right. It is the same on every call.
pub(crate) fn sample_notebook() -> String {
    todo!()
}

/// Writes [`sample_notebook`] to `path`.
pub(crate) fn write_sample(path: &Path) -> io::Result<()> {
    todo!()
}

/// How long each protocol takes, from decoding a noisy PNG of the given size
/// to the data for the terminal, at 80 columns and cells of 10 by 20 pixels.
/// Nothing is written to the terminal.
pub(crate) fn run_bench(width: u32, height: u32) -> Vec<(ProtocolType, Duration)> {
    todo!()
}
