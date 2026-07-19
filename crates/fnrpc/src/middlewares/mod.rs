//! Built-in middleware implementations.
//!
//! - [`hook::HookLayer`] — before/after hooks for simple logic.
//! - [`tracing::TracingLayer`] — structured logging (feature = `"tracing"`).

pub mod hook;
#[cfg(feature = "tracing")]
pub mod tracing;
