//! Ergonomic NSURLSession bindings for Rust
//!
//! This crate provides async-only bindings to Apple's NSURLSession API,
//! designed to work seamlessly with tokio.

#![cfg_attr(docsrs, feature(doc_cfg))]
#![deny(missing_docs)]

#[cfg(not(target_vendor = "apple"))]
compile_error!("rsurlsession only supports Apple platforms");

pub use client::Client;
pub use error::{Error, Result};
pub use request::{Request, RequestBuilder};
pub use response::Response;

mod client;
mod error;
mod request;
mod response;
mod session;
mod task;
mod delegate;
mod body;
