//! T-503: feedback loop + weights/decay.
//!
//! Covers every acceptance edge: good raises (cap 3.0), bad lowers/floors and
//! never creates, `--executed`/`--accepted` flags, deterministic decay with
//! `FakeClock`, note redaction, unknown ids, empty terms, and multi-expansion
//! bumps. Both `InMemoryStore` and `SqliteStore` run the same assertions.

use std::fs;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use shx_core::{Interaction, PrunePolicy, RiskLevel, Verdict, VocabEntry, VocabSource};
use shx_memory::{InMemoryStore, MemoryStore, SqliteStore, vocab};

static DIR_COUNTER: AtomicU64 = AtomicU64::new(0);

fn sample(ts: i64, input: &str) -> Interaction {
    Interaction {
        id: None,
        ts,
        session_id: "s".into(),
        project_id: None,
        cwd: "/tmp".into(),
        os: "macos".into(),
        shell: "zsh".into(),
        input_nl: input.into(),
        output_cmd: "docker run pg".into(),
        explanation: None,
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: Some(0.9),
        latency_ms: 10,
        risk_level: RiskLevel::Safe,
        risk_notes: vec![],
        from_cache: false,
        accepted: None,
        executed: None,
        tags: vec![],
    }
}

fn taught(term: &str, expansion: &str, weight: f64, now: i64) -> VocabEntry {
    VocabEntry {
        term: term.into(),
        expansion: expansion.into(),
        weight,
        source: VocabSource::Taught,
        last_used_ts: now,
        use_count: 1,
    }
}

fn unique_dir(suffix: &str) -> std::path::PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let n = DIR_COUNTER.fetch_add(1, Ordering::SeqCst);
    let tid = format!("{:?}", std::thread::current().id());
    let dir = std::env::temp_dir().join(format!(
        "shx-t503-{suffix}-{}-{nanos}-{n}-{}",
        std::process::id(),
        tid.replace(['(', ')', ' ', ','], "_")
    ));
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn weight_of(store: &dyn MemoryStore, term: &str, expansion: &str) -> Option<VocabEntry> {
    store
        .vocabulary(&[term.to_string()])
        .unwrap()
        .into_iter()
        .find(|e| e.expansion == expansion)
}

#[test]
fn t_503_good_raises_weight_capped_at_3() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    for store in [&mem as &dyn MemoryStore, &sqlite as &dyn MemoryStore] {
        // Single-token intent so only `pg` is touched (no `run`/`7000` noise).
        let id = store.record_interaction(&sample(100, "pg")).unwrap();
        store
            .upsert_vocabulary(&taught("pg", "postgres", 2.0, 100))
            .unwrap();
        // Exercise the frozen trait path; repeated goods must cap at 3.0.
        store.feedback(id, Verdict::Good, None).unwrap();
        let w1 = weight_of(store, "pg", "postgres").unwrap().weight;
        assert!((w1 - 2.5).abs() < 1e-9, "first good 2.0 -> 2.5, got {w1}");
        store.feedback(id, Verdict::Good, None).unwrap();
        let w2 = weight_of(store, "pg", "postgres").unwrap().weight;
        assert!((w2 - 3.0).abs() < 1e-9, "second good caps at 3.0, got {w2}");
        store.feedback(id, Verdict::Good, None).unwrap();
        let w3 = weight_of(store, "pg", "postgres").unwrap().weight;
        assert!(
            (w3 - 3.0).abs() < 1e-9,
            "further goods never exceed 3.0, got {w3}"
        );
    }

    // Deterministic clock path: last_used moves to now.
    let fresh = InMemoryStore::default();
    let clock = vocab::FakeClock::new(5_000);
    let id = fresh.record_interaction(&sample(101, "pg")).unwrap();
    fresh
        .upsert_vocabulary(&taught("pg", "postgres", 1.0, 1))
        .unwrap();
    let touched = fresh
        .feedback_at(id, Verdict::Good, None, clock.now())
        .unwrap();
    assert_eq!(touched, 1);
    let e = weight_of(&fresh, "pg", "postgres").unwrap();
    assert!((e.weight - 1.5).abs() < 1e-9);
    assert_eq!(e.last_used_ts, clock.now());
    assert!(vocab::is_applied(&e));
}

#[test]
fn t_503_bad_lowers_floored_and_never_creates() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    for store in [&mem as &dyn MemoryStore, &sqlite as &dyn MemoryStore] {
        let id = store.record_interaction(&sample(200, "run pg")).unwrap();
        store
            .upsert_vocabulary(&taught("pg", "postgres", 2.0, 200))
            .unwrap();
        store.feedback(id, Verdict::Bad, None).unwrap();
        let w1 = weight_of(store, "pg", "postgres").unwrap();
        assert!((w1.weight - 1.5).abs() < 1e-9, "bad 2.0 -> 1.5");
        assert_eq!(w1.use_count, 1, "bad is not a use");
        // Drive to the floor; never negative.
        for _ in 0..10 {
            store.feedback(id, Verdict::Bad, None).unwrap();
        }
        let floored = weight_of(store, "pg", "postgres").unwrap();
        assert!(
            (floored.weight - vocab::WEIGHT_FLOOR).abs() < 1e-9,
            "floored at 0.0, got {}",
            floored.weight
        );
        assert!(floored.weight >= vocab::WEIGHT_FLOOR);
    }

    // Bad on a brand-new term creates nothing (each store separately, since
    // feedback_at is inherent, not on the frozen trait).
    let mem2 = InMemoryStore::default();
    let mid = mem2
        .record_interaction(&sample(201, "run zzbrandnewterm"))
        .unwrap();
    assert_eq!(
        mem2.feedback_at(mid, Verdict::Bad, None, 999).unwrap(),
        0,
        "bad must not create vocabulary"
    );
    assert!(
        mem2.vocabulary(&["zzbrandnewterm".to_string()])
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        mem2.get_interaction(mid).unwrap().unwrap().accepted,
        Some(false)
    );

    let sqlite2 = SqliteStore::open_in_memory().unwrap();
    let sid = sqlite2
        .record_interaction(&sample(201, "run zzbrandnewterm"))
        .unwrap();
    assert_eq!(
        sqlite2.feedback_at(sid, Verdict::Bad, None, 999).unwrap(),
        0,
        "bad must not create vocabulary"
    );
    assert!(
        sqlite2
            .vocabulary(&["zzbrandnewterm".to_string()])
            .unwrap()
            .is_empty()
    );
    assert_eq!(sqlite2.get(sid).unwrap().unwrap().accepted, Some(false));
}

#[test]
fn t_503_good_creates_learned_and_needs_three_to_apply() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    // Single-token intent so only `k8s` is touched. InMemory deterministic path.
    let id = mem.record_interaction(&sample(300, "k8s")).unwrap();
    let t1 = mem.feedback_at(id, Verdict::Good, None, 301).unwrap();
    assert_eq!(t1, 1);
    let e1 = weight_of(&mem, "k8s", "k8s").unwrap();
    assert_eq!(e1.source, VocabSource::Learned);
    assert!((e1.weight - 0.5).abs() < 1e-9);
    assert!(!vocab::is_applied(&e1), "one-off is not applied");
    mem.feedback_at(id, Verdict::Good, None, 302).unwrap();
    let e2 = weight_of(&mem, "k8s", "k8s").unwrap();
    assert!((e2.weight - 1.0).abs() < 1e-9);
    assert!(!vocab::is_applied(&e2));
    mem.feedback_at(id, Verdict::Good, None, 303).unwrap();
    let e3 = weight_of(&mem, "k8s", "k8s").unwrap();
    assert!((e3.weight - 1.5).abs() < 1e-9);
    assert!(vocab::is_applied(&e3), "third good reaches apply threshold");
    assert_eq!(e3.use_count, 3);

    // SQLite same lifecycle through the frozen trait (wall clock).
    let sid = sqlite.record_interaction(&sample(300, "k8s")).unwrap();
    sqlite.feedback(sid, Verdict::Good, None).unwrap();
    let s1 = weight_of(&sqlite, "k8s", "k8s").unwrap();
    assert!((s1.weight - 0.5).abs() < 1e-9);
    assert!(!vocab::is_applied(&s1));
}

#[test]
fn t_503_multi_term_feedback_touches_every_token() {
    // Documents the multi-token rule: every retriever token in the intent is
    // eligible. `run pg on 7000` -> ["7000", "pg", "run"].
    let mem = InMemoryStore::default();
    let id = mem
        .record_interaction(&sample(350, "run pg on 7000"))
        .unwrap();
    mem.upsert_vocabulary(&taught("pg", "postgres", 2.0, 350))
        .unwrap();
    let touched = mem.feedback_at(id, Verdict::Good, None, 351).unwrap();
    assert_eq!(touched, 3, "pg + run + 7000");
    assert!((weight_of(&mem, "pg", "postgres").unwrap().weight - 2.5).abs() < 1e-9);
    assert!((weight_of(&mem, "run", "run").unwrap().weight - 0.5).abs() < 1e-9);
    assert!((weight_of(&mem, "7000", "7000").unwrap().weight - 0.5).abs() < 1e-9);
}

#[test]
fn t_503_taught_beats_learned_after_feedback() {
    let mem = InMemoryStore::default();
    let id = mem.record_interaction(&sample(400, "pg")).unwrap();
    mem.upsert_vocabulary(&taught("pg", "postgres", 2.0, 400))
        .unwrap();
    mem.upsert_vocabulary(&VocabEntry {
        term: "pg".into(),
        expansion: "postgresql".into(),
        weight: 3.0,
        source: VocabSource::Learned,
        last_used_ts: 400,
        use_count: 9,
    })
    .unwrap();
    mem.feedback_at(id, Verdict::Good, None, 401).unwrap();
    let mut rows = mem.vocabulary(&["pg".to_string()]).unwrap();
    rows.sort_by(vocab::cmp_rank);
    assert_eq!(rows[0].source, VocabSource::Taught);
    assert_eq!(rows[0].expansion, "postgres");
    assert!((rows[0].weight - 2.5).abs() < 1e-9);
    assert!((rows[1].weight - 3.0).abs() < 1e-9, "learned stays capped");
}

#[test]
fn t_503_executed_and_accepted_flags() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    let mid = mem.record_interaction(&sample(500, "pg")).unwrap();
    mem.feedback_at(mid, Verdict::Good, None, 501).unwrap();
    let row = mem.get_interaction(mid).unwrap().unwrap();
    assert_eq!(row.accepted, Some(true));
    assert_eq!(row.executed, None);
    mem.set_executed(mid, true).unwrap();
    let row = mem.get_interaction(mid).unwrap().unwrap();
    assert_eq!(row.executed, Some(true));
    assert_eq!(
        row.accepted,
        Some(true),
        "executed must not clobber accepted"
    );
    mem.set_accepted(mid, false).unwrap();
    assert_eq!(
        mem.get_interaction(mid).unwrap().unwrap().accepted,
        Some(false)
    );
    mem.set_accepted(mid, true).unwrap();
    assert_eq!(
        mem.get_interaction(mid).unwrap().unwrap().accepted,
        Some(true)
    );

    let sid = sqlite.record_interaction(&sample(500, "pg")).unwrap();
    sqlite.feedback_at(sid, Verdict::Bad, None, 501).unwrap();
    assert_eq!(sqlite.get(sid).unwrap().unwrap().accepted, Some(false));
    sqlite.set_executed(sid, true).unwrap();
    let row = sqlite.get(sid).unwrap().unwrap();
    assert_eq!(row.executed, Some(true));
    assert_eq!(row.accepted, Some(false));
    // --accepted forces accepted=1 even after a bad verdict.
    sqlite.set_accepted(sid, true).unwrap();
    assert_eq!(sqlite.get(sid).unwrap().unwrap().accepted, Some(true));
}

#[test]
fn t_503_feedback_unknown_id_errors() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    for msg in [
        mem.feedback_at(9999, Verdict::Good, None, 1)
            .unwrap_err()
            .to_string(),
        sqlite
            .feedback_at(9999, Verdict::Good, None, 1)
            .unwrap_err()
            .to_string(),
        mem.set_executed(9999, true).unwrap_err().to_string(),
        sqlite.set_executed(9999, true).unwrap_err().to_string(),
        mem.set_accepted(9999, true).unwrap_err().to_string(),
        sqlite.set_accepted(9999, true).unwrap_err().to_string(),
    ] {
        assert!(msg.contains("no interaction 9999"), "got: {msg}");
    }
    assert!(mem.get_interaction(9999).unwrap().is_none());
    assert!(sqlite.get(9999).unwrap().is_none());
}

#[test]
fn t_503_note_is_redacted_before_store() {
    let dir = unique_dir("note");
    let path = dir.join("shx.db");
    let store = SqliteStore::open(&path).unwrap();
    let id = store.record_interaction(&sample(600, "run pg")).unwrap();
    let secret = "sk-TESTFAKE0000000000000000";
    let note = format!("token {secret} leaked?");
    store
        .feedback_at(id, Verdict::Good, Some(&note), 601)
        .unwrap();
    drop(store);
    let conn = rusqlite::Connection::open(&path).unwrap();
    let stored: String = conn
        .query_row(
            "SELECT note FROM feedback WHERE interaction_id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .unwrap();
    assert!(
        !stored.contains(secret),
        "raw secret must not land in feedback.note: {stored}"
    );
    assert!(
        stored.contains("«redacted:"),
        "expected redaction marker: {stored}"
    );

    // InMemory ignores the note but still records acceptance + learning.
    let mem = InMemoryStore::default();
    let mid = mem.record_interaction(&sample(600, "run pg")).unwrap();
    mem.feedback_at(mid, Verdict::Good, Some(&note), 601)
        .unwrap();
    assert_eq!(
        mem.get_interaction(mid).unwrap().unwrap().accepted,
        Some(true)
    );
}

#[test]
fn t_503_empty_terms_touch_nothing_but_still_accepts() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();
    assert!(vocab::extract_terms("the and or").is_empty());

    let mid = mem.record_interaction(&sample(700, "the and or")).unwrap();
    let touched = mem.feedback_at(mid, Verdict::Good, None, 701).unwrap();
    assert_eq!(touched, 0);
    assert!(mem.vocabulary(&[]).unwrap().is_empty());
    assert_eq!(
        mem.get_interaction(mid).unwrap().unwrap().accepted,
        Some(true)
    );

    let sid = sqlite
        .record_interaction(&sample(700, "the and or"))
        .unwrap();
    let touched = sqlite.feedback_at(sid, Verdict::Good, None, 701).unwrap();
    assert_eq!(touched, 0);
    assert!(sqlite.vocabulary(&[]).unwrap().is_empty());
}

#[test]
fn t_503_multiple_expansions_all_bumped() {
    let mem = InMemoryStore::default();
    let id = mem.record_interaction(&sample(800, "pg")).unwrap();
    mem.upsert_vocabulary(&taught("pg", "postgres", 2.0, 800))
        .unwrap();
    mem.upsert_vocabulary(&taught("pg", "postgresql", 1.0, 800))
        .unwrap();
    // Second taught row needs Learned source to differ; fix one to learned.
    let mut rows = mem.vocabulary(&["pg".to_string()]).unwrap();
    rows.sort_by(|a, b| a.expansion.cmp(&b.expansion));
    assert_eq!(rows.len(), 2);
    let touched = mem.feedback_at(id, Verdict::Good, None, 801).unwrap();
    assert_eq!(touched, 2, "every expansion for the term bumps");
    let pg = weight_of(&mem, "pg", "postgres").unwrap();
    let pg2 = weight_of(&mem, "pg", "postgresql").unwrap();
    assert!((pg.weight - 2.5).abs() < 1e-9);
    assert!((pg2.weight - 1.5).abs() < 1e-9);

    let bad_touched = mem.feedback_at(id, Verdict::Bad, None, 802).unwrap();
    assert_eq!(bad_touched, 2);
    assert!((weight_of(&mem, "pg", "postgres").unwrap().weight - 2.0).abs() < 1e-9);
}

#[test]
fn t_503_decay_deterministic_after_feedback() {
    let clock = vocab::FakeClock::new(10_000);
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();

    for (is_mem, id) in [
        (true, mem.record_interaction(&sample(900, "pg")).unwrap()),
        (
            false,
            sqlite.record_interaction(&sample(900, "pg")).unwrap(),
        ),
    ] {
        if is_mem {
            mem.upsert_vocabulary(&VocabEntry {
                term: "pg".into(),
                expansion: "postgres".into(),
                weight: 2.0,
                source: VocabSource::Learned,
                last_used_ts: clock.now(),
                use_count: 1,
            })
            .unwrap();
            mem.feedback_at(id, Verdict::Good, None, clock.now())
                .unwrap();
            let w0 = weight_of(&mem, "pg", "postgres").unwrap().weight;
            assert!((w0 - 2.5).abs() < 1e-9);
            clock.advance(vocab::IDLE_MS);
            let report = mem
                .prune_at(
                    &PrunePolicy {
                        retention_days: 3650,
                        keep_danger: true,
                    },
                    clock.now(),
                )
                .unwrap();
            assert_eq!(report.vocab_decayed, 1);
            let w1 = weight_of(&mem, "pg", "postgres").unwrap().weight;
            assert!((w1 - 2.5 * vocab::DECAY).abs() < 1e-9);
        } else {
            sqlite
                .upsert_vocabulary(&VocabEntry {
                    term: "pg".into(),
                    expansion: "postgres".into(),
                    weight: 2.0,
                    source: VocabSource::Learned,
                    last_used_ts: clock.now(),
                    use_count: 1,
                })
                .unwrap();
            // Use a fixed now (not wall clock) so the assertion is stable.
            sqlite
                .feedback_at(id, Verdict::Good, None, clock.now())
                .unwrap();
            let w0 = weight_of(&sqlite, "pg", "postgres").unwrap().weight;
            assert!((w0 - 2.5).abs() < 1e-9);
            let report = sqlite
                .prune_at(
                    &PrunePolicy {
                        retention_days: 3650,
                        keep_danger: true,
                    },
                    clock.now() + vocab::IDLE_MS,
                )
                .unwrap();
            assert_eq!(report.vocab_decayed, 1);
        }
    }
}

#[test]
fn t_503_trait_feedback_sets_accepted_and_learns() {
    let mem = InMemoryStore::default();
    let sqlite = SqliteStore::open_in_memory().unwrap();
    for store in [&mem as &dyn MemoryStore, &sqlite as &dyn MemoryStore] {
        let id = store.record_interaction(&sample(1000, "run pg")).unwrap();
        store
            .upsert_vocabulary(&taught("pg", "postgres", 2.0, 1000))
            .unwrap();
        store.feedback(id, Verdict::Good, None).unwrap();
        let row = store
            .recent(10, shx_core::Scope::Tool)
            .unwrap()
            .into_iter()
            .find(|i| i.id == Some(id))
            .unwrap();
        assert_eq!(row.accepted, Some(true));
        assert!((weight_of(store, "pg", "postgres").unwrap().weight - 2.5).abs() < 1e-9);
        store.feedback(id, Verdict::Bad, None).unwrap();
        let row = store
            .recent(10, shx_core::Scope::Tool)
            .unwrap()
            .into_iter()
            .find(|i| i.id == Some(id))
            .unwrap();
        assert_eq!(row.accepted, Some(false));
        assert!((weight_of(store, "pg", "postgres").unwrap().weight - 2.0).abs() < 1e-9);
    }
}
