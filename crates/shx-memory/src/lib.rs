//! Persistent memory store and retrieval for `shx`.
//!
//! Depends inward on `shx-core` only.

#![forbid(unsafe_code)]

pub mod memory;
pub mod paths;
pub mod record;
pub mod sqlite;
pub mod store;

pub use memory::InMemoryStore;
pub use sqlite::SqliteStore;
pub use store::{MemoryError, MemoryStore, Result};
