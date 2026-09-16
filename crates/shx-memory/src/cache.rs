//! Fast-path cache (docs/03-MEMORY.md §5, ADR-005).
//!
//! Keyed by normalized intent + context fingerprint.
//! Eligibility rules (from 03-MEMORY.md §5):
//! - Risk level must be `RiskLevel::Safe` (never `Danger` or `Review`).
//! - `accepted = 1` (or unmarked but repeated 2+ times with the same command).
//! - Context fingerprint matches: hash of `(normalized_intent, os, shell, cwd, project_id)`.

use std::collections::HashMap;

use shx_core::{Interaction, RiskLevel};

use crate::store::{MemoryError, Result};

/// Query parameters for fast-path cache lookup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CacheQuery<'a> {
    /// Raw intent text from user.
    pub intent: &'a str,
    /// Operating system string ("macos", "linux", "windows").
    pub os: &'a str,
    /// Shell name ("zsh", "bash", etc.).
    pub shell: &'a str,
    /// Working directory path.
    pub cwd: &'a str,
    /// Project identifier hash (from git root), if within a project.
    pub project_id: Option<&'a str>,
    /// Whether mock backend entries may be served (e.g. `--offline`).
    pub allow_mock: bool,
    /// Whether `--local` routing is forced.
    pub force_local: bool,
    /// Whether `--cloud` routing is forced.
    pub force_cloud: bool,
}

/// A cached command matching intent and context fingerprint.
#[derive(Debug, Clone, PartialEq)]
pub struct CacheHit {
    /// Shell command string.
    pub command: String,
    /// Optional natural language explanation.
    pub explanation: Option<String>,
    /// Backend provider that originated the translation.
    pub backend: String,
    /// Model name that originated the translation.
    pub model: String,
    /// Confidence score (always 1.0 for eligible cache hits per 06-BACKENDS.md §6).
    pub confidence: Option<f32>,
    /// Risk level (always `Safe` for cache hits).
    pub risk_level: RiskLevel,
    /// Rule IDs hit (empty for safe).
    pub risk_notes: Vec<String>,
}

/// Normalize an intent string: trim, collapse whitespace, and lowercase.
pub fn normalize_intent(intent: &str) -> String {
    intent
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

/// Compute a deterministic 64-bit context fingerprint using FNV-1a.
///
/// Hashes `(normalized_intent, os, shell, cwd, project_id)` per 03-MEMORY.md §5.
pub fn compute_context_fingerprint(
    normalized_intent: &str,
    os: &str,
    shell: &str,
    cwd: &str,
    project_id: Option<&str>,
) -> u64 {
    let normalized_cwd = shx_core::env::normalize_git_root(cwd);
    let mut bytes = Vec::new();
    bytes.extend_from_slice(normalized_intent.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(os.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(shell.as_bytes());
    bytes.push(0);
    bytes.extend_from_slice(normalized_cwd.as_bytes());
    bytes.push(0);
    if let Some(pid) = project_id {
        bytes.extend_from_slice(pid.as_bytes());
    }
    shx_core::env::fnv1a_hash(&bytes)
}

/// Compute context fingerprint for a recorded interaction.
pub fn interaction_context_fingerprint(i: &Interaction) -> u64 {
    compute_context_fingerprint(
        &normalize_intent(&i.input_nl),
        &i.os,
        &i.shell,
        &i.cwd,
        i.project_id.as_deref(),
    )
}

/// Validate whether a risk level is eligible to be cached.
///
/// Rejects `Danger` and `Review`. Under 03-MEMORY.md §5 and ADR-002, the cache
/// never stores danger results.
pub fn validate_cacheable(risk: RiskLevel) -> Result<()> {
    if risk != RiskLevel::Safe {
        return Err(MemoryError::Message(format!(
            "results with risk level {risk:?} cannot be cached; only Safe results are cache-eligible"
        )));
    }
    Ok(())
}

/// Check if a risk level is eligible for the fast-path cache.
pub fn is_cacheable_risk(risk: RiskLevel) -> bool {
    risk == RiskLevel::Safe
}

/// Check whether a command is cache-eligible given all occurrences matching the fingerprint.
///
/// Rules (03-MEMORY.md §5):
/// 1. If the latest interaction was explicitly rejected (`accepted == Some(false)`), not eligible.
/// 2. If any interaction has `accepted == Some(true)` and the latest was not rejected, eligible.
/// 3. If unmarked (`accepted.is_none()`), eligible only if repeated 2+ times with no rejections.
pub fn is_command_eligible(events: &[&Interaction]) -> bool {
    if events.is_empty() {
        return false;
    }
    // Check latest verdict
    if events.first().is_some_and(|l| l.accepted == Some(false)) {
        return false;
    }
    let has_accepted = events.iter().any(|e| e.accepted == Some(true));
    if has_accepted {
        return true;
    }
    // Count unmarked or accepted occurrences (ignoring rejected)
    let non_rejected_count = events.iter().filter(|e| e.accepted != Some(false)).count();
    non_rejected_count >= 2
}

/// Pure lookup over an interaction slice.
///
/// Returns the newest eligible cached command matching the query's normalized intent
/// and context fingerprint.
pub fn find_cache_hit_in_interactions(
    interactions: &[Interaction],
    query: &CacheQuery,
) -> Option<CacheHit> {
    let norm_intent = normalize_intent(query.intent);
    if norm_intent.is_empty() {
        return None;
    }
    let query_fp = compute_context_fingerprint(
        &norm_intent,
        query.os,
        query.shell,
        query.cwd,
        query.project_id,
    );

    // Filter to safe interactions matching context fingerprint and routing constraints
    let matching: Vec<&Interaction> = interactions
        .iter()
        .filter(|i| is_cacheable_risk(i.risk_level))
        .filter(|i| interaction_context_fingerprint(i) == query_fp)
        .filter(|i| {
            if !query.allow_mock && i.backend == "mock" {
                return false;
            }
            if query.force_local
                && i.backend != "ollama"
                && (!query.allow_mock || i.backend != "mock")
            {
                return false;
            }
            if query.force_cloud && (i.backend == "ollama" || i.backend == "mock") {
                return false;
            }
            true
        })
        .collect();

    if matching.is_empty() {
        return None;
    }

    // Group by output command
    let mut by_cmd: HashMap<&str, Vec<&Interaction>> = HashMap::new();
    for i in &matching {
        by_cmd.entry(&i.output_cmd).or_default().push(i);
    }

    // Find eligible commands and choose the one with the latest timestamp
    let mut best: Option<(&Interaction, i64)> = None;

    for occurrences in by_cmd.values_mut() {
        // Sort descending by ts
        occurrences.sort_by_key(|b| std::cmp::Reverse(b.ts));
        if is_command_eligible(occurrences) {
            let Some(latest) = occurrences.first() else {
                continue;
            };
            match best {
                None => best = Some((latest, latest.ts)),
                Some((_, best_ts)) if latest.ts > best_ts => {
                    best = Some((latest, latest.ts));
                }
                _ => {}
            }
        }
    }

    best.map(|(hit, _)| CacheHit {
        command: hit.output_cmd.clone(),
        explanation: hit.explanation.clone(),
        backend: hit.backend.clone(),
        model: hit.model.clone(),
        confidence: Some(1.0),
        risk_level: RiskLevel::Safe,
        risk_notes: vec![],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_intent() {
        assert_eq!(
            normalize_intent("  run   pg   on  7000  "),
            "run pg on 7000"
        );
        assert_eq!(normalize_intent("GIT STATUS"), "git status");
    }

    #[test]
    fn test_fingerprint_distinctness() {
        let fp1 = compute_context_fingerprint("status", "macos", "zsh", "/repo/a", Some("p1"));
        let fp2 = compute_context_fingerprint("status", "macos", "zsh", "/repo/b", Some("p1"));
        let fp3 = compute_context_fingerprint("status", "macos", "zsh", "/repo/a", Some("p2"));
        let fp4 = compute_context_fingerprint("status", "linux", "zsh", "/repo/a", Some("p1"));
        let fp5 = compute_context_fingerprint("status", "macos", "bash", "/repo/a", Some("p1"));

        assert_ne!(fp1, fp2, "different cwd must produce different fingerprint");
        assert_ne!(
            fp1, fp3,
            "different project must produce different fingerprint"
        );
        assert_ne!(fp1, fp4, "different os must produce different fingerprint");
        assert_ne!(
            fp1, fp5,
            "different shell must produce different fingerprint"
        );
    }

    #[test]
    fn test_validate_cacheable_rejects_danger() {
        assert!(validate_cacheable(RiskLevel::Safe).is_ok());
        assert!(validate_cacheable(RiskLevel::Review).is_err());
        assert!(validate_cacheable(RiskLevel::Danger).is_err());
    }
}
