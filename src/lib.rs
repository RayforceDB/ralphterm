//! Internal library for the `ralphterm` and `ralphex` binaries.
//!
//! No items in this crate are part of a stable public API. The library
//! exists only to let the two binaries share the CLI entry point. If you
//! depend on this crate as a library, expect breaking changes between
//! every release.

#[doc(hidden)]
pub mod cli;
#[doc(hidden)]
pub mod config;
#[doc(hidden)]
pub mod docker;
#[doc(hidden)]
pub mod notify;
#[doc(hidden)]
pub mod output_format;
#[doc(hidden)]
pub mod plan;
#[doc(hidden)]
pub mod preflight;
#[doc(hidden)]
pub mod prompts;
#[doc(hidden)]
pub mod pty_agent;
#[doc(hidden)]
pub mod runner;
#[doc(hidden)]
pub mod runs;
#[doc(hidden)]
pub mod signals;
#[doc(hidden)]
pub mod store;
#[doc(hidden)]
pub mod workspace;
