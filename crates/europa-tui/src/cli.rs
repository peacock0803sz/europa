//! Command line arguments.

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;

use crate::exit::EX_USAGE;

/// Arguments of the viewer.
#[derive(clap::Parser)]
#[command(name = "europa", about = "View a Jupyter notebook in the terminal")]
pub(crate) struct Args {
    /// The notebook to open
    pub(crate) notebook: PathBuf,
}

/// Read the notebook path from `args`.
pub(crate) fn parse(args: impl IntoIterator<Item = OsString>) -> Result<PathBuf, ExitCode> {
    use clap::Parser;
    match Args::try_parse_from(args) {
        Ok(got) => Ok(got.notebook),
        Err(error) if error.kind() == clap::error::ErrorKind::DisplayHelp => {
            print!("{}", error.render());
            Err(ExitCode::SUCCESS)
        }
        Err(error) => {
            eprint!("{}", error.render());
            Err(ExitCode::from(EX_USAGE))
        }
    }
}
