//! Persistent memory store and retrieval for `shx`.
//!
//! Depends inward on `shx-core` only.

#![forbid(unsafe_code)]

pub mod store;

pub use store::{InMemoryStore, MemoryError, MemoryStore, Result};
