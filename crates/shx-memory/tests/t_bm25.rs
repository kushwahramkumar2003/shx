//! T-505 ContextBuilder wiring: BM25 selection is visible in the bundle,
//! and two builds of the same store stay byte-identical.

use shx_core::{EnvInfo, Interaction, Profile, RiskLevel, SecretRedactor, VocabEntry, VocabSource};
use shx_memory::{ContextBudget, ContextBuilder, InMemoryStore, MemoryStore};

fn env() -> EnvInfo {
    EnvInfo {
        os: "macos".into(),
        shell: "zsh".into(),
        cwd: "/tmp".into(),
        git_root: None,
        in_container: false,
    }
}

fn profile() -> Profile {
    Profile {
        name: "default".into(),
        ports: vec![7000],
        prefer_docker: true,
        notes: String::new(),
    }
}

fn ix(ts: i64, input: &str, cmd: &str) -> Interaction {
    Interaction {
        id: None,
        ts,
        session_id: "s".into(),
        project_id: Some("p1".into()),
        cwd: "/tmp".into(),
        os: "macos".into(),
        shell: "zsh".into(),
        input_nl: input.into(),
        output_cmd: cmd.into(),
        explanation: None,
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: None,
        latency_ms: 1,
        risk_level: RiskLevel::Safe,
        risk_notes: vec![],
        from_cache: false,
        accepted: None,
        executed: None,
        tags: vec![],
    }
}

/// Bundle history is recency-ordered (spec step 6); step 2 only *selects*.
/// With `recent=0`, history is the BM25 top-n set. Relevant rows must appear.
#[test]
fn t_505_context_builder_selects_relevant_hits() {
    let store = InMemoryStore::default();
    for i in 0..10 {
        store
            .record_interaction(&ix(100 + i, &format!("run job {i}"), "true"))
            .unwrap();
    }
    store
        .record_interaction(&ix(
            50,
            "start postgres locally",
            "docker run -p 5432:5432 postgres",
        ))
        .unwrap();
    store
        .record_interaction(&ix(51, "follow api pod logs", "kubectl logs -f deploy/api"))
        .unwrap();
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

    let b = ContextBudget {
        recent: 0,
        relevance: 5,
        shell: 0,
        max_tokens: 1500,
    };
    let builder = ContextBuilder::new(&SecretRedactor);

    let pg = builder
        .build("run postgres", &env(), &profile(), &store, b, None)
        .unwrap();
    assert!(
        pg.history
            .iter()
            .any(|i| i.input_nl == "start postgres locally"),
        "BM25 should select the postgres row; got {:?}",
        pg.history
            .iter()
            .map(|i| i.input_nl.as_str())
            .collect::<Vec<_>>()
    );

    let shorthand = builder
        .build("pg", &env(), &profile(), &store, b, None)
        .unwrap();
    assert!(
        shorthand
            .history
            .iter()
            .any(|i| i.input_nl == "start postgres locally"),
        "vocab expansion should retrieve postgres; got {:?}",
        shorthand
            .history
            .iter()
            .map(|i| i.input_nl.as_str())
            .collect::<Vec<_>>()
    );

    // Pre-T-505 `search(full intent)` misses paraphrases; BM25 should not.
    let search_hits = store
        .search("run postgres", 8, shx_core::Scope::Tool)
        .unwrap();
    assert!(
        search_hits
            .iter()
            .all(|i| i.input_nl != "start postgres locally"),
        "sanity: old search(intent) should miss the paraphrase"
    );
}

#[test]
fn t_505_bm25_bundle_is_byte_deterministic() {
    let store = InMemoryStore::default();
    for i in 0..15 {
        store
            .record_interaction(&ix(
                i,
                &format!("run job {i} postgres extra"),
                &format!("echo {i}"),
            ))
            .unwrap();
    }
    let b = ContextBudget {
        recent: 3,
        relevance: 5,
        shell: 0,
        max_tokens: 400,
    };
    let builder = ContextBuilder::new(&SecretRedactor);
    let a = builder
        .build("run postgres", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    let c = builder
        .build("run postgres", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    assert_eq!(a, c);
}
