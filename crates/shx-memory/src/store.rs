//! Frozen [`MemoryStore`] trait (docs/02-ARCHITECTURE.md §4).

use shx_core::{Interaction, PrunePolicy, PruneReport, Scope, Snippet, Verdict, VocabEntry};

/// Persistence errors.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// SQLite / SQL failure.
    #[error("sqlite: {0}")]
    Sqlite(String),
    /// Filesystem failure.
    #[error("{0}")]
    Io(String),
    /// Schema or data that cannot be interpreted.
    #[error("{0}")]
    Message(String),
}

impl From<rusqlite::Error> for MemoryError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e.to_string())
    }
}

/// Result alias for store methods.
pub type Result<T> = std::result::Result<T, MemoryError>;

/// Read/write API over tool history, vocabulary, snippets, and prune.
pub trait MemoryStore {
    /// Insert one interaction; returns the assigned id.
    fn record_interaction(&self, i: &Interaction) -> Result<i64>;

    /// Newest-first interactions in `scope`, capped at `limit`.
    fn recent(&self, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;

    /// Keyword search over `scope`.
    fn search(&self, query: &str, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;

    /// Vocabulary rows whose `term` is in `terms`.
    fn vocabulary(&self, terms: &[String]) -> Result<Vec<VocabEntry>>;

    /// Insert or update a vocabulary row.
    fn upsert_vocabulary(&self, e: &VocabEntry) -> Result<()>;

    /// All snippets, unordered.
    fn snippets(&self) -> Result<Vec<Snippet>>;

    /// Record user feedback against an interaction.
    fn feedback(&self, interaction_id: i64, v: Verdict, note: Option<&str>) -> Result<()>;

    /// Apply retention and danger-command rules.
    fn prune(&self, policy: &PrunePolicy) -> Result<PruneReport>;
}
