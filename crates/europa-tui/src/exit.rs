//! Exit codes from `sysexits.h`.

/// Command line usage error, for missing or extra arguments.
pub(crate) const EX_USAGE: u8 = 64;
/// Data error, for a notebook that cannot be parsed.
pub(crate) const EX_DATAERR: u8 = 65;
/// Input error, for a file that cannot be read.
pub(crate) const EX_NOINPUT: u8 = 66;
/// IO error, for a terminal that cannot be used.
pub(crate) const EX_IOERR: u8 = 74;
