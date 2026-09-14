//! Vocabulary teach / learn / apply rules (docs/03-MEMORY.md §5).

use shx_core::{VocabEntry, VocabSource};

/// Taught rows start here.
pub const TAUGHT_WEIGHT: f64 = 2.0;
/// Implicit bump on `good` feedback.
pub const LEARN_DELTA: f64 = 0.5;
/// Cap for learned (and taught) weight.
pub const WEIGHT_CAP: f64 = 3.0;
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
}
