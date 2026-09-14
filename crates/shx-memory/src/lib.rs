//! Persistent memory store and retrieval for `shx`.
//!
//! Depends inward on `shx-core` only.

#![forbid(unsafe_code)]

pub mod memory;
pub mod paths;
pub mod record;
pub mod retrieve;
pub mod sqlite;
pub mod store;

pub use memory::InMemoryStore;
pub use retrieve::{ContextBudget, ContextBuilder, memory_tokens};
pub use sqlite::SqliteStore;
pub use store::{MemoryError, MemoryStore, Result};
