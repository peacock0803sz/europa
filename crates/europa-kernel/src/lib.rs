//! Kernel adapter for Europa.
//!
//! The crate exposes the [`KernelClient`] window, the local kernel behind
//! it, and [`kill_all_kernels`] for emergency cleanup. Callers drive kernels
//! through the trait and read [`europa_core::KernelEvent`] values from the
//! `std::sync::mpsc` channel, so the UI can swap the real kernel for a scripted one
//! without changing its code.
#![deny(missing_docs)]

mod client;
mod connection;
mod convert;
mod handshake;
mod local;
mod process;
mod registry;
mod wire;

pub use client::KernelClient;
pub use local::LocalKernel;
pub use registry::kill_all_kernels;
