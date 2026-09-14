//! T-MEM-2: budget cap, determinism, fixtures, redaction.

use shx_core::{
    EnvInfo, Interaction, Profile, RiskLevel, SecretRedactor, Snippet, VocabEntry, VocabSource,
};
use shx_memory::{ContextBudget, ContextBuilder, InMemoryStore, MemoryStore, memory_tokens};

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

/// T-MEM-2: exact bundle for a fixed store; two builds match; budget is a cap.
#[test]
fn t_mem_2_fixture_deterministic_budget() {
    let store = InMemoryStore::default();
    store
        .record_interaction(&ix(300, "run pg on 7000", "docker run pg"))
        .unwrap();
    store
        .record_interaction(&ix(200, "list files", "ls -la"))
        .unwrap();
    store
        .record_interaction(&ix(100, "cargo test", "cargo test"))
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
    store
        .insert_snippet(Snippet {
            id: None,
            name: "pg-up".into(),
            command: "docker run postgres".into(),
            description: Some("start postgres".into()),
            created_ts: 1,
            use_count: 0,
        })
        .unwrap();

    let b = ContextBudget {
        recent: 10,
        relevance: 5,
        shell: 5,
        max_tokens: 1500,
    };
    let builder = ContextBuilder::new(&SecretRedactor);
    let a = builder
        .build("run pg on 7000", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    let c = builder
        .build("run pg on 7000", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    assert_eq!(a, c);
    assert_eq!(
        a.history
            .iter()
            .map(|i| i.input_nl.as_str())
            .collect::<Vec<_>>(),
        ["run pg on 7000", "list files", "cargo test"]
    );
    assert!(a.vocabulary.iter().any(|v| v.term == "pg"));
    assert!(a.snippets.iter().any(|s| s.name == "pg-up"));
    assert!(memory_tokens(&a) <= b.max_tokens as usize);
}

#[test]
fn t_mem_2_never_exceeds_max_tokens() {
    let store = InMemoryStore::default();
    for i in 0..40 {
        store
            .record_interaction(&ix(
                i,
                &format!("intent number {i} with extra padding words"),
                &format!("echo {i} {}", "x".repeat(80)),
            ))
            .unwrap();
    }
    let b = ContextBudget {
        recent: 20,
        relevance: 10,
        shell: 0,
        max_tokens: 30,
    };
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build("intent number extra", &env(), &profile(), &store, b, None)
        .unwrap();
    assert!(
        memory_tokens(&bundle) <= 30,
        "tokens {}",
        memory_tokens(&bundle)
    );
}

#[test]
fn t_mem_2_redacts_history_blocks() {
    let store = InMemoryStore::default();
    store
        .record_interaction(&ix(1, "export TOKEN=sk-TESTFAKE0000000000000000", "true"))
        .unwrap();
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build(
            "export TOKEN",
            &env(),
            &profile(),
            &store,
            ContextBudget::default(),
            None,
        )
        .unwrap();
    for i in &bundle.history {
        assert!(
            !i.input_nl.contains("sk-TEST"),
            "secret leaked: {}",
            i.input_nl
        );
    }
}

#[test]
fn taught_beats_learned_and_one_off_not_applied() {
    let store = InMemoryStore::default();
    store
        .upsert_vocabulary(&shx_memory::vocab::taught("pg", "postgres", 1))
        .unwrap();
    store
        .upsert_vocabulary(&VocabEntry {
            term: "pg".into(),
            expansion: "postgresql".into(),
            weight: 3.0,
            source: VocabSource::Learned,
            last_used_ts: 1,
            use_count: 9,
        })
        .unwrap();
    store
        .upsert_vocabulary(&VocabEntry {
            term: "k8s".into(),
            expansion: "k8s".into(),
            weight: 0.5,
            source: VocabSource::Learned,
            last_used_ts: 1,
            use_count: 1,
        })
        .unwrap();
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build(
            "run pg on k8s",
            &env(),
            &profile(),
            &store,
            ContextBudget::default(),
            None,
        )
        .unwrap();
    let pgs: Vec<_> = bundle
        .vocabulary
        .iter()
        .filter(|v| v.term == "pg")
        .collect();
    assert_eq!(pgs.len(), 1);
    assert_eq!(pgs[0].expansion, "postgres");
    assert!(!bundle.vocabulary.iter().any(|v| v.term == "k8s"));
}

#[test]
fn t_mem_2_empty_store_ok() {
    let store = InMemoryStore::default();
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build(
            "",
            &env(),
            &profile(),
            &store,
            ContextBudget::default(),
            None,
        )
        .unwrap();
    assert!(bundle.history.is_empty());
    assert!(memory_tokens(&bundle) <= 1500);
}

/// T-MEM-2 golden: BM25 relevance prefers the rare-term hit over common-term
/// recency distractors. `recent=2` so the two newest noise rows are the anchor;
/// the postgres row is older and must come from step 2.
#[test]
fn t_mem_2_bm25_relevance_golden() {
    let store = InMemoryStore::default();
    store
        .record_interaction(&ix(1000, "noise newest", "true"))
        .unwrap();
    store
        .record_interaction(&ix(900, "noise second", "true"))
        .unwrap();
    store
        .record_interaction(&ix(800, "noise third", "true"))
        .unwrap();
    store
        .record_interaction(&ix(700, "start postgres locally", "pg_ctl start"))
        .unwrap();
    store
        .record_interaction(&ix(600, "run the tests", "cargo test"))
        .unwrap();
    for i in 0..8 {
        store
            .record_interaction(&ix(100 + i, &format!("run job {i}"), "true"))
            .unwrap();
    }

    let b = ContextBudget {
        recent: 2,
        relevance: 1,
        shell: 0,
        max_tokens: 1500,
    };
    let builder = ContextBuilder::new(&SecretRedactor);
    let a = builder
        .build("run postgres", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    let c = builder
        .build("run postgres", &env(), &profile(), &store, b, Some("p1"))
        .unwrap();
    assert_eq!(a, c);
    assert_eq!(
        a.history
            .iter()
            .map(|i| i.input_nl.as_str())
            .collect::<Vec<_>>(),
        ["noise newest", "noise second", "start postgres locally"]
    );
}

/// Vocabulary expansions join the BM25 query so `pg` retrieves `postgres` rows.
#[test]
fn t_mem_2_bm25_expands_vocabulary() {
    let store = InMemoryStore::default();
    store
        .record_interaction(&ix(
            50,
            "start postgres locally",
            "brew services start postgresql",
        ))
        .unwrap();
    store
        .record_interaction(&ix(40, "list files", "ls -la"))
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
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build("pg", &env(), &profile(), &store, b, None)
        .unwrap();
    assert_eq!(
        bundle
            .history
            .iter()
            .map(|i| i.input_nl.as_str())
            .collect::<Vec<_>>(),
        ["start postgres locally"]
    );
}

/// Unrelated history must not fill relevance slots (zero BM25 dropped).
#[test]
fn t_mem_2_bm25_drops_zero_scores() {
    let store = InMemoryStore::default();
    store
        .record_interaction(&ix(3, "list files", "ls -la"))
        .unwrap();
    store
        .record_interaction(&ix(2, "cargo test", "cargo test"))
        .unwrap();
    store
        .record_interaction(&ix(1, "git status", "git status"))
        .unwrap();
    let b = ContextBudget {
        recent: 0,
        relevance: 5,
        shell: 0,
        max_tokens: 1500,
    };
    let bundle = ContextBuilder::new(&SecretRedactor)
        .build("zzzznonexistent", &env(), &profile(), &store, b, None)
        .unwrap();
    assert!(bundle.history.is_empty());
}
