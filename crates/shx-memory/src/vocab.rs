//! Vocabulary teach / learn / apply rules (docs/03-MEMORY.md §5).

use shx_core::{VocabEntry, VocabSource};

/// Taught rows start here.
pub const TAUGHT_WEIGHT: f64 = 2.0;
/// Implicit bump on `good` feedback.
pub const LEARN_DELTA: f64 = 0.5;
/// Decrement on `bad` feedback (floored at [`WEIGHT_FLOOR`]).
pub const BAD_DELTA: f64 = 0.5;
/// Cap for learned (and taught) weight.
pub const WEIGHT_CAP: f64 = 3.0;
/// Floor for `bad` feedback; weights never go negative.
pub const WEIGHT_FLOOR: f64 = 0.0;
/// Learned rows are applied in prompts only at or above this.
pub const APPLY_MIN_WEIGHT: f64 = 1.5;
/// Per-prune decay when idle ≥ 30 days.
pub const DECAY: f64 = 0.98;
/// Idle window for decay, milliseconds.
pub const IDLE_MS: i64 = 30 * 86_400_000;

/// Controllable clock for deterministic decay tests.
#[derive(Debug)]
pub struct FakeClock {
    /// Unix milliseconds.
    pub now_ms: std::sync::atomic::AtomicI64,
}

impl FakeClock {
    /// Start at `now_ms`.
    pub fn new(now_ms: i64) -> Self {
        Self {
            now_ms: std::sync::atomic::AtomicI64::new(now_ms),
        }
    }

    /// Current time.
    pub fn now(&self) -> i64 {
        self.now_ms.load(std::sync::atomic::Ordering::SeqCst)
    }

    /// Advance by `delta_ms`.
    pub fn advance(&self, delta_ms: i64) {
        self.now_ms
            .fetch_add(delta_ms, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Build a taught row (`weight = 2.0`).
pub fn taught(term: &str, expansion: &str, now_ms: i64) -> VocabEntry {
    VocabEntry {
        term: term.trim().to_ascii_lowercase(),
        expansion: expansion.trim().to_string(),
        weight: TAUGHT_WEIGHT,
        source: VocabSource::Taught,
        last_used_ts: now_ms,
        use_count: 0,
    }
}

/// Whether this row is injected into prompts.
pub fn is_applied(e: &VocabEntry) -> bool {
    e.source == VocabSource::Taught || e.weight >= APPLY_MIN_WEIGHT
}

/// Taught rows sort before learned; then higher weight.
pub fn cmp_rank(a: &VocabEntry, b: &VocabEntry) -> std::cmp::Ordering {
    let a_t = u8::from(a.source != VocabSource::Taught);
    let b_t = u8::from(b.source != VocabSource::Taught);
    a_t.cmp(&b_t)
        .then(b.weight.total_cmp(&a.weight))
        .then(a.term.cmp(&b.term))
        .then(a.expansion.cmp(&b.expansion))
}

/// Terms in `input` that can carry feedback learning.
///
/// Same tokenization as the context builder (`BM25` unique tokens: lowercase,
/// stopword-filtered, single-char tokens dropped), so `good`/`bad` only ever
/// touches terms the retriever could have used. Deterministic and sorted.
pub fn extract_terms(input: &str) -> Vec<String> {
    crate::bm25::unique_tokens(input)
}

/// Apply one `good` verdict to a term.
///
/// New terms start as `Learned` at `0.5` (not applied until `1.5`, so one-off
/// coincidences never become vocabulary). Existing rows (taught or learned)
/// gain `+0.5` capped at `3.0`; taught rows keep their source and therefore
/// keep outranking learned rows. `last_used_ts` always moves to `now_ms` so
/// prune-time decay stays deterministic.
pub fn apply_good(existing: Option<&VocabEntry>, term: &str, now_ms: i64) -> VocabEntry {
    learn_bump(existing, term, now_ms)
}

/// Apply one `bad` verdict to an existing row.
///
/// Weight drops by `0.5` floored at `0.0` and never rises. Source is
/// preserved (a bad mark does not demote a taught row to learned).
/// `last_used_ts` moves to `now_ms` so decay stays deterministic;
/// `use_count` is left alone because a rejection is not a use.
/// There is no `Option` form on purpose: `bad` never creates vocabulary.
pub fn apply_bad(existing: &VocabEntry, now_ms: i64) -> VocabEntry {
    let mut out = existing.clone();
    out.weight = (existing.weight - BAD_DELTA).max(WEIGHT_FLOOR);
    out.last_used_ts = now_ms;
    out
}

/// Bump or insert a learned row. Does not change `Taught` source.
pub fn learn_bump(existing: Option<&VocabEntry>, term: &str, now_ms: i64) -> VocabEntry {
    if let Some(e) = existing {
        let mut out = e.clone();
        out.weight = (e.weight + LEARN_DELTA).min(WEIGHT_CAP);
        out.last_used_ts = now_ms;
        out.use_count = e.use_count.saturating_add(1);
        return out;
    }
    VocabEntry {
        term: term.trim().to_ascii_lowercase(),
        expansion: term.trim().to_ascii_lowercase(),
        weight: LEARN_DELTA,
        source: VocabSource::Learned,
        last_used_ts: now_ms,
        use_count: 1,
    }
}

/// Apply one idle-window decay if `last_used` is old enough.
pub fn decay_if_idle(e: &mut VocabEntry, now_ms: i64) -> bool {
    if now_ms.saturating_sub(e.last_used_ts) >= IDLE_MS {
        e.weight *= DECAY;
        true
    } else {
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn taught_beats_learned_on_conflict() {
        let taught = taught("pg", "postgres", 1);
        let learned = VocabEntry {
            term: "pg".into(),
            expansion: "postgresql".into(),
            weight: 3.0,
            source: VocabSource::Learned,
            last_used_ts: 1,
            use_count: 9,
        };
        let mut v = [learned.clone(), taught.clone()];
        v.sort_by(cmp_rank);
        assert_eq!(v[0].source, VocabSource::Taught);
        assert_eq!(v[0].expansion, "postgres");
    }

    #[test]
    fn one_off_good_is_not_applied() {
        let e = learn_bump(None, "k8s", 1);
        assert_eq!(e.source, VocabSource::Learned);
        assert!((e.weight - LEARN_DELTA).abs() < f64::EPSILON);
        assert!(!is_applied(&e));
        let e2 = learn_bump(Some(&e), "k8s", 2);
        assert!(!is_applied(&e2));
        let e3 = learn_bump(Some(&e2), "k8s", 3);
        assert!(is_applied(&e3));
        assert!(e3.weight >= APPLY_MIN_WEIGHT);
    }

    #[test]
    fn fake_clock_decay_is_deterministic() {
        let clock = FakeClock::new(1_000);
        let mut e = taught("pg", "postgres", clock.now());
        e.source = VocabSource::Learned;
        e.weight = 2.0;
        assert!(!decay_if_idle(&mut e, clock.now()));
        clock.advance(IDLE_MS);
        assert!(decay_if_idle(&mut e, clock.now()));
        assert!((e.weight - 2.0 * DECAY).abs() < 1e-9);
        let w = e.weight;
        clock.advance(IDLE_MS);
        decay_if_idle(&mut e, clock.now());
        assert!((e.weight - w * DECAY).abs() < 1e-9);
    }

    #[test]
    fn good_caps_at_3_and_keeps_taught_source() {
        let mut e = taught("pg", "postgres", 1);
        for now in 2..10 {
            e = apply_good(Some(&e), "pg", now);
        }
        assert!((e.weight - WEIGHT_CAP).abs() < 1e-9, "capped: {}", e.weight);
        assert_eq!(e.source, VocabSource::Taught);
        assert_eq!(e.last_used_ts, 9);
        // One more good never exceeds the cap.
        let capped = apply_good(Some(&e), "pg", 10);
        assert!((capped.weight - WEIGHT_CAP).abs() < 1e-9);
    }

    #[test]
    fn bad_lowers_but_never_below_floor_and_keeps_source() {
        let base = VocabEntry {
            term: "pg".into(),
            expansion: "postgres".into(),
            weight: 2.0,
            source: VocabSource::Taught,
            last_used_ts: 1,
            use_count: 4,
        };
        let lowered = apply_bad(&base, 2);
        assert!((lowered.weight - 1.5).abs() < 1e-9);
        assert_eq!(lowered.source, VocabSource::Taught);
        assert_eq!(lowered.last_used_ts, 2);
        assert_eq!(lowered.use_count, 4, "bad is not a use");
        let mut e = VocabEntry {
            weight: 0.2,
            ..base.clone()
        };
        for now in 3..10 {
            e = apply_bad(&e, now);
        }
        assert!(
            (e.weight - WEIGHT_FLOOR).abs() < 1e-9,
            "floored: {}",
            e.weight
        );
        assert!(e.weight >= WEIGHT_FLOOR);
    }

    #[test]
    fn bad_never_creates_and_good_needs_three_to_apply() {
        // No constructor for bad-without-existing by design; document via types.
        let e1 = apply_good(None, "k8s", 1);
        assert_eq!(e1.weight, LEARN_DELTA);
        assert!(!is_applied(&e1));
        let e2 = apply_good(Some(&e1), "k8s", 2);
        assert!(!is_applied(&e2));
        let e3 = apply_good(Some(&e2), "k8s", 3);
        assert!(is_applied(&e3));
        // A bad mark drops it back below the apply threshold.
        let e4 = apply_bad(&e3, 4);
        assert!(!is_applied(&e4));
    }

    #[test]
    fn extract_terms_matches_retriever_tokenization() {
        assert_eq!(extract_terms("run pg on 7000"), vec!["7000", "pg", "run"]);
        assert_eq!(extract_terms("the and or"), Vec::<String>::new());
        assert_eq!(extract_terms("a I x"), Vec::<String>::new());
        assert_eq!(extract_terms("Run PG, run PG!"), vec!["pg", "run"]);
        assert_eq!(extract_terms(""), Vec::<String>::new());
    }
}
