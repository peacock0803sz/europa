//! Terminal interface for Europa.
//!
//! The library draws what `europa-render` makes of a cell, including images
//! of PNG and JPEG outputs.
#![deny(missing_docs)]
// Nothing outside the tests reaches these items until the image probe is
// wired in, and the first drafts of some functions do not use their
// arguments. The allowance goes with that wiring.
#![allow(dead_code, unused_variables)]

mod image_cache;
