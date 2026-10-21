//! Shared data types for Europa core models.
#![deny(missing_docs)]

pub mod error;
pub mod io;
pub mod normalize;
pub mod notebook;
pub mod pos;

pub use error::ParseError;
pub use normalize::MultilineText;
