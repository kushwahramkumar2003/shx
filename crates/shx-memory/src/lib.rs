//! Persistent memory store and retrieval for `shx`.
//!
//! Depends inward on `shx-core` only.

#![forbid(unsafe_code)]

mod bm25;
pub mod cache;
pub mod memory;
pub mod paths;
pub mod record;
pub mod retrieve;
pub mod scope;
pub mod sqlite;
pub mod store;
pub mod vocab;

pub use cache::{
    CacheHit, CacheQuery, compute_context_fingerprint, normalize_intent, validate_cacheable,
};
pub use memory::InMemoryStore;
pub use retrieve::{ContextBudget, ContextBuilder, memory_tokens};
pub use scope::{
    detect_in_container, find_git_root, recall_project_history, resolve_project_scope,
};
pub use sqlite::SqliteStore;
pub use store::{MemoryError, MemoryStore, Result};
