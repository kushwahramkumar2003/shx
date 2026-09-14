//! Frozen [`MemoryStore`] trait and an in-memory stub.
//!
//! The SQLite implementation and a complete [`InMemoryStore`] land in T-201.
//! This stub exists so CLI and pipeline tasks can compile against the trait.

use shx_core::{Interaction, PrunePolicy, PruneReport, Scope, Snippet, Verdict, VocabEntry};

/// Persistence errors. Concrete variants grow in T-201.
#[derive(Debug, thiserror::Error)]
pub enum MemoryError {
    /// Catch-all until the SQLite impl lands.
    #[error("{0}")]
    Message(String),
    /// Method not yet implemented by this store.
    #[error("not implemented")]
    Unimplemented,
}

/// Result alias for store methods.
pub type Result<T> = std::result::Result<T, MemoryError>;

/// Read/write API over tool history, vocabulary, snippets, and prune.
///
/// `Scope` is what lets "tool history only" and "also shell history" coexist
/// behind one API (ADR-003).
pub trait MemoryStore {
    /// Insert one interaction; returns the assigned id.
    fn record_interaction(&self, i: &Interaction) -> Result<i64>;

    /// Newest-first interactions in `scope`, capped at `limit`.
    fn recent(&self, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;

    /// Keyword/relevance search over `scope`.
    fn search(&self, query: &str, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;

    /// Vocabulary rows whose `term` is in `terms` (plus high-weight defaults
    /// once T-201 lands).
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

/// Empty in-memory double. Full behavior (conformance suite) is T-201.
#[derive(Debug, Default)]
pub struct InMemoryStore;

impl MemoryStore for InMemoryStore {
    fn record_interaction(&self, _i: &Interaction) -> Result<i64> {
        Err(MemoryError::Unimplemented)
    }

    fn recent(&self, _limit: usize, _scope: Scope) -> Result<Vec<Interaction>> {
        Ok(Vec::new())
    }

    fn search(&self, _query: &str, _limit: usize, _scope: Scope) -> Result<Vec<Interaction>> {
        Ok(Vec::new())
    }

    fn vocabulary(&self, _terms: &[String]) -> Result<Vec<VocabEntry>> {
        Ok(Vec::new())
    }

    fn upsert_vocabulary(&self, _e: &VocabEntry) -> Result<()> {
        Err(MemoryError::Unimplemented)
    }

    fn snippets(&self) -> Result<Vec<Snippet>> {
        Ok(Vec::new())
    }

    fn feedback(&self, _interaction_id: i64, _v: Verdict, _note: Option<&str>) -> Result<()> {
        Err(MemoryError::Unimplemented)
    }

    fn prune(&self, _policy: &PrunePolicy) -> Result<PruneReport> {
        Ok(PruneReport::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stub_recent_is_empty() {
        let store = InMemoryStore;
        let rows = store.recent(10, Scope::Tool).expect("recent");
        assert!(rows.is_empty());
    }

    #[test]
    fn stub_record_is_unimplemented() {
        let store = InMemoryStore;
        let err = store
            .record_interaction(&Interaction {
                id: None,
                ts: 0,
                session_id: "s".into(),
                project_id: None,
                cwd: "/".into(),
                os: "macos".into(),
                shell: "zsh".into(),
                input_nl: "x".into(),
                output_cmd: "true".into(),
                explanation: None,
                backend: "mock".into(),
                model: "fixture".into(),
                confidence: None,
                latency_ms: 0,
                risk_level: shx_core::RiskLevel::Safe,
                risk_notes: vec![],
                from_cache: false,
                accepted: None,
                executed: None,
                tags: vec![],
            })
            .unwrap_err();
        assert!(matches!(err, MemoryError::Unimplemented));
    }
}
