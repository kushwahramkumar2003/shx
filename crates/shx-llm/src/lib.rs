//! LLM backend integrations for `shx`.
//!
//! Depends inward on `shx-core` only. No async runtime (ADR-006).

#![forbid(unsafe_code)]

pub mod backend;

pub use backend::{Backend, BackendError, Capabilities, CostTier, ErrorKind, Health};
