//! Kernel adapter for Europa.
//!
//! The crate exposes the [`KernelClient`] window and the local kernel behind
//! it. Callers drive kernels only through the trait and read
//! [`europa_core::KernelEvent`] values from the standard channel, so the UI can swap the real kernel for a
//! scripted one without changing its code.
#![deny(missing_docs)]

mod client;
mod connection;
#[allow(dead_code)]
mod convert;
mod handshake;
mod local;
mod process;
#[allow(dead_code)]
mod wire;

pub use client::KernelClient;
pub use local::LocalKernel;
