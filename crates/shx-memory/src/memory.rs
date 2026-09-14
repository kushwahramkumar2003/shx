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
        let want: Vec<String> = terms.iter().map(|t| t.to_ascii_lowercase()).collect();
        let g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
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
        let mut g = self
            .inner
            .lock()
            .map_err(|e| MemoryError::Message(e.to_string()))?;
        let now = now_ms();
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
            if now.saturating_sub(v.last_used_ts) >= 30 * DAY_MS {
                v.weight *= 0.98;
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

fn in_scope(i: &Interaction, scope: &Scope) -> bool {
    match scope {
        Scope::Tool => true,
        Scope::Shell => false,
        Scope::Project { id: Some(pid) } => i.project_id.as_deref() == Some(pid.as_str()),
        Scope::Project { id: None } => i.project_id.is_some(),
    }
}
