//! Kernel adapter for Europa.
//!
//! The crate exposes the [`KernelClient`] window and the local kernel behind
//! it. Callers drive kernels only through the trait and read
//! [`europa_core::KernelEvent`] values from the standard channel, so the UI can swap the real kernel for a
//! scripted one without changing its code.
#![deny(missing_docs)]

mod client;
#[allow(dead_code)]
mod connection;
#[allow(dead_code)]
mod convert;
#[allow(dead_code)]
mod handshake;
#[allow(dead_code)]
mod local;
#[allow(dead_code)]
mod process;
#[allow(dead_code)]
mod wire;

pub use client::KernelClient;
