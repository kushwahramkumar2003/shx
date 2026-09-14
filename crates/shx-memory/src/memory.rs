//! Full in-memory [`MemoryStore`] (tests and `--offline`).

use std::sync::Mutex;

use shx_core::{
    Interaction, PrunePolicy, PruneReport, RiskLevel, Scope, Snippet, Verdict, VocabEntry,
};

use crate::store::{MemoryError, MemoryStore, Result};

struct Inner {
    next_id: i64,
    interactions: Vec<Interaction>,
    vocab: Vec<VocabEntry>,
    snippets: Vec<Snippet>,
}

/// Process-local store. Same behavior as SQLite for the conformance suite.
#[derive(Default)]
pub struct InMemoryStore {
    inner: Mutex<Inner>,
}

impl Default for Inner {
    fn default() -> Self {
        Self {
            next_id: 1,
            interactions: Vec::new(),
            vocab: Vec::new(),
            snippets: Vec::new(),
        }
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

const DAY_MS: i64 = 86_400_000;

impl InMemoryStore {
    /// Insert or replace a snippet by unique name (redacted). Not on the frozen trait.
    pub fn insert_snippet(&self, s: Snippet) -> Result<()> {
        self.upsert_snippet(&s).map(|_| ())
    }

    /// Insert or replace a snippet by unique name (redacted). Not on the frozen trait.
    pub fn upsert_snippet(&self, s: &Snippet) -> Result<i64> {
        let mut s = crate::record::prepare_snippet(s);
        if s.name.is_empty() {
            return Err(MemoryError::Message("snippet name is empty".into()));
        }
        if s.command.trim().is_empty() {
            return Err(MemoryError::Message("snippet command is empty".into()));
        }
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        if let Some(existing) = g.snippets.iter_mut().find(|e| e.name == s.name) {
            existing.command = s.command;
            existing.description = s.description;
            return Ok(existing.id.unwrap_or(0));
        }
        let id = g.next_id;
        g.next_id += 1;
        s.id = Some(id);
        g.snippets.push(s);
        Ok(id)
    }

    /// Fetch one snippet by unique name.
    pub fn get_snippet(&self, name: &str) -> Result<Option<Snippet>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        Ok(g.snippets.iter().find(|s| s.name == name).cloned())
    }

    /// Delete a snippet by unique name. Returns rows removed.
    pub fn delete_snippet(&self, name: &str) -> Result<u64> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let before = g.snippets.len();
        g.snippets.retain(|s| s.name != name);
        Ok((before - g.snippets.len()) as u64)
    }

    /// Delete vocabulary rows for `term`.
    pub fn forget_vocabulary(&self, term: &str) -> Result<u64> {
        let t = term.trim().to_ascii_lowercase();
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let before = g.vocab.len();
        g.vocab.retain(|e| e.term != t);
        Ok((before - g.vocab.len()) as u64)
    }

    /// Prune using an explicit timestamp (FakeClock).
    pub fn prune_at(&self, policy: &PrunePolicy, now: i64) -> Result<PruneReport> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let cutoff = now.saturating_sub(i64::from(policy.retention_days) * DAY_MS);
        let danger_cut = now.saturating_sub(30 * DAY_MS);
        let before = g.interactions.len();
        g.interactions.retain(|i| i.ts >= cutoff);
        let deleted = (before - g.interactions.len()) as u64;
        let mut stripped = 0u64;
        if !policy.keep_danger {
            for i in &mut g.interactions {
                if i.risk_level == RiskLevel::Danger
                    && i.ts < danger_cut
                    && !i.output_cmd.is_empty()
                {
                    i.output_cmd.clear();
                    stripped += 1;
                }
            }
        }
        let mut decayed = 0u64;
        for v in &mut g.vocab {
            if crate::vocab::decay_if_idle(v, now) {
                decayed += 1;
            }
        }
        Ok(PruneReport {
            interactions_deleted: deleted,
            vocab_decayed: decayed,
            danger_commands_stripped: stripped,
        })
    }
}

impl MemoryStore for InMemoryStore {
    fn record_interaction(&self, i: &Interaction) -> Result<i64> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let id = g.next_id;
        g.next_id += 1;
        let mut row = crate::record::prepare_interaction(i);
        row.id = Some(id);
        g.interactions.push(row);
        Ok(id)
    }

    fn recent(&self, limit: usize, scope: Scope) -> Result<Vec<Interaction>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let mut rows: Vec<Interaction> = g
            .interactions
            .iter()
            .filter(|i| in_scope(i, &scope))
            .cloned()
            .collect();
        rows.sort_by(|a, b| b.ts.cmp(&a.ts).then(b.id.cmp(&a.id)));
        rows.truncate(limit);
        Ok(rows)
    }

    fn search(&self, query: &str, limit: usize, scope: Scope) -> Result<Vec<Interaction>> {
        let q = query.to_ascii_lowercase();
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let mut rows: Vec<Interaction> = g
            .interactions
            .iter()
            .filter(|i| in_scope(i, &scope))
            .filter(|i| {
                i.input_nl.to_ascii_lowercase().contains(&q)
                    || i.output_cmd.to_ascii_lowercase().contains(&q)
            })
            .cloned()
            .collect();
        rows.sort_by_key(|b| std::cmp::Reverse(b.ts));
        rows.truncate(limit);
        Ok(rows)
    }

    fn vocabulary(&self, terms: &[String]) -> Result<Vec<VocabEntry>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        if terms.is_empty() {
            let mut all = g.vocab.clone();
            all.sort_by(|a, b| b.weight.total_cmp(&a.weight));
            return Ok(all);
        }
        let want: Vec<String> = terms.iter().map(|t| t.to_ascii_lowercase()).collect();
        Ok(g.vocab
            .iter()
            .filter(|e| want.iter().any(|t| t == &e.term))
            .cloned()
            .collect())
    }

    fn upsert_vocabulary(&self, e: &VocabEntry) -> Result<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        if let Some(existing) = g
            .vocab
            .iter_mut()
            .find(|v| v.term == e.term && v.expansion == e.expansion)
        {
            *existing = e.clone();
        } else {
            g.vocab.push(e.clone());
        }
        Ok(())
    }

    fn snippets(&self) -> Result<Vec<Snippet>> {
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        Ok(g.snippets.clone())
    }

    fn feedback(&self, interaction_id: i64, v: Verdict, _note: Option<&str>) -> Result<()> {
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let row = g
            .interactions
            .iter_mut()
            .find(|i| i.id == Some(interaction_id))
            .ok_or_else(|| MemoryError::Message(format!("no interaction {interaction_id}")))?;
        row.accepted = Some(matches!(v, Verdict::Good));
        Ok(())
    }

    fn prune(&self, policy: &PrunePolicy) -> Result<PruneReport> {
        self.prune_at(policy, now_ms())
    }
}

fn in_scope(i: &Interaction, scope: &Scope) -> bool {
    match scope {
        Scope::Tool => true,
        Scope::Shell => false,
        Scope::Project { id: Some(pid) } => i.project_id.as_deref() == Some(pid.as_str()),
        Scope::Project { id: None } => i.project_id.is_some(),
    }
}
