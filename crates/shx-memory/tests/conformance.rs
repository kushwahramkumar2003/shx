//! Shared MemoryStore conformance (InMemory + SQLite).

use shx_core::{Interaction, PrunePolicy, RiskLevel, Scope, Verdict, VocabEntry, VocabSource};
use shx_memory::{InMemoryStore, MemoryStore, SqliteStore};

fn sample(ts: i64, input: &str, cmd: &str, project: Option<&str>, risk: RiskLevel) -> Interaction {
    Interaction {
        id: None,
        ts,
        session_id: "s".into(),
        project_id: project.map(str::to_string),
        cwd: "/tmp".into(),
        os: "macos".into(),
        shell: "zsh".into(),
        input_nl: input.into(),
        output_cmd: cmd.into(),
        explanation: None,
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: Some(0.9),
        latency_ms: 10,
        risk_level: risk,
        risk_notes: vec![],
        from_cache: false,
        accepted: None,
        executed: None,
        tags: vec!["t".into()],
    }
}

fn run_suite(store: &dyn MemoryStore) {
    let a = store
        .record_interaction(&sample(
            100,
            "run pg",
            "docker run pg",
            Some("p1"),
            RiskLevel::Safe,
        ))
        .unwrap();
    let _b = store
        .record_interaction(&sample(200, "list files", "ls", None, RiskLevel::Safe))
        .unwrap();

    let recent = store.recent(10, Scope::Tool).unwrap();
    assert_eq!(recent.len(), 2);
    assert_eq!(recent[0].input_nl, "list files");
    assert_eq!(recent[1].input_nl, "run pg");
    assert_eq!(recent[1].id, Some(a));

    let proj = store
        .recent(
            10,
            Scope::Project {
                id: Some("p1".into()),
            },
        )
        .unwrap();
    assert_eq!(proj.len(), 1);
    assert_eq!(proj[0].input_nl, "run pg");

    let found = store.search("pg", 10, Scope::Tool).unwrap();
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].output_cmd, "docker run pg");

    store
        .upsert_vocabulary(&VocabEntry {
            term: "pg".into(),
            expansion: "postgres".into(),
            weight: 2.0,
            source: VocabSource::Taught,
            last_used_ts: 1,
            use_count: 1,
        })
        .unwrap();
    let vocab = store.vocabulary(&["pg".into()]).unwrap();
    assert_eq!(vocab.len(), 1);
    assert_eq!(vocab[0].expansion, "postgres");

    store.feedback(a, Verdict::Good, Some("nice")).unwrap();
    let again = store.recent(10, Scope::Tool).unwrap();
    let row = again.iter().find(|i| i.id == Some(a)).unwrap();
    assert_eq!(row.accepted, Some(true));

    let old = now_ms() - 40 * 86_400_000;
    store
        .record_interaction(&sample(old, "wipe", "rm -rf /", None, RiskLevel::Danger))
        .unwrap();
    let report = store
        .prune(&PrunePolicy {
            retention_days: 3650,
            keep_danger: false,
        })
        .unwrap();
    assert_eq!(report.danger_commands_stripped, 1);
    let danger = store.search("wipe", 10, Scope::Tool).unwrap();
    assert_eq!(danger[0].output_cmd, "");

    let ancient = now_ms() - 400 * 86_400_000;
    store
        .record_interaction(&sample(ancient, "stale", "true", None, RiskLevel::Safe))
        .unwrap();
    let report = store
        .prune(&PrunePolicy {
            retention_days: 30,
            keep_danger: true,
        })
        .unwrap();
    assert!(report.interactions_deleted >= 1);
    let stale = store.search("stale", 10, Scope::Tool).unwrap();
    assert!(stale.is_empty());
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64
}

#[test]
fn in_memory_conformance() {
    run_suite(&InMemoryStore::default());
}

#[test]
fn sqlite_memory_conformance() {
    run_suite(&SqliteStore::open_in_memory().expect("mem sqlite"));
}
