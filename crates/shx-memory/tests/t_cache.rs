//! Acceptance tests for T-502 / T-MEM-4: Fast-path cache.
//!
//! Invariants:
//! - T-MEM-4: Same intent, different cwd/project_id -> no cache hit.
//! - Cache-hit path < 5 ms in a loop.
//! - Cache never stores or returns danger results.
//! - Cache eligibility: Safe risk, accepted=1 or unmarked repeated 2+ times.

use std::env;
use std::fs;
use std::path::PathBuf;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use shx_core::{Interaction, RiskLevel};
use shx_memory::{CacheQuery, InMemoryStore, MemoryStore, SqliteStore, validate_cacheable};

fn unique_dir(prefix: &str) -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = env::temp_dir().join(format!("shx-test-cache-{prefix}-{nanos}"));
    fs::create_dir_all(&dir).expect("temp dir");
    dir
}

fn make_interaction(
    id: i64,
    input: &str,
    cmd: &str,
    cwd: &str,
    project_id: Option<&str>,
    risk: RiskLevel,
    accepted: Option<bool>,
) -> Interaction {
    Interaction {
        id: Some(id),
        ts: 1_000_000 + id,
        session_id: "test-session".into(),
        project_id: project_id.map(str::to_string),
        cwd: cwd.into(),
        os: "macos".into(),
        shell: "zsh".into(),
        input_nl: input.into(),
        output_cmd: cmd.into(),
        explanation: Some(format!("Runs {cmd}")),
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: Some(1.0),
        latency_ms: 1,
        risk_level: risk,
        risk_notes: vec![],
        from_cache: false,
        accepted,
        executed: None,
        tags: vec![],
    }
}

/// T-MEM-4: Same intent, different cwd/project_id -> no cache hit.
/// Verifies both InMemoryStore and SqliteStore.
#[test]
fn t_mem_4_cache_safety_different_cwd_or_project_no_hit() {
    let dir = unique_dir("t-mem-4");
    let db_path = dir.join("cache.db");
    let sqlite = SqliteStore::open(&db_path).expect("open sqlite");
    let in_mem = InMemoryStore::default();

    // Interaction recorded in /proj/a with project_id = "proj-a-hash", accepted = true
    let item = make_interaction(
        1,
        "docker run pg",
        "docker run -d -p 5432:5432 postgres",
        "/proj/a",
        Some("proj-a-hash"),
        RiskLevel::Safe,
        Some(true),
    );

    sqlite.record_interaction(&item).expect("record sqlite");
    in_mem.record_interaction(&item).expect("record in_mem");

    // 1. Exact match query -> HIT
    let exact_q = CacheQuery {
        intent: "docker run pg",
        os: "macos",
        shell: "zsh",
        cwd: "/proj/a",
        project_id: Some("proj-a-hash"),
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    let hit_sqlite = sqlite.cache_lookup(&exact_q).expect("sqlite query");
    assert!(
        hit_sqlite.is_some(),
        "exact context must hit cache in sqlite"
    );
    assert_eq!(
        hit_sqlite.unwrap().command,
        "docker run -d -p 5432:5432 postgres"
    );

    let hit_in_mem = in_mem.cache_lookup(&exact_q).expect("in_mem query");
    assert!(
        hit_in_mem.is_some(),
        "exact context must hit cache in in_mem"
    );

    // 2. Different CWD (/proj/b) with same project_id -> NO HIT
    let diff_cwd_q = CacheQuery {
        intent: "docker run pg",
        os: "macos",
        shell: "zsh",
        cwd: "/proj/b",
        project_id: Some("proj-a-hash"),
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite
            .cache_lookup(&diff_cwd_q)
            .expect("sqlite query")
            .is_none(),
        "T-MEM-4 violation: different cwd must NEVER hit cache in sqlite"
    );
    assert!(
        in_mem
            .cache_lookup(&diff_cwd_q)
            .expect("in_mem query")
            .is_none(),
        "T-MEM-4 violation: different cwd must NEVER hit cache in in_mem"
    );

    // 3. Same CWD (/proj/a) with different project_id ("proj-b-hash") -> NO HIT
    let diff_proj_q = CacheQuery {
        intent: "docker run pg",
        os: "macos",
        shell: "zsh",
        cwd: "/proj/a",
        project_id: Some("proj-b-hash"),
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite
            .cache_lookup(&diff_proj_q)
            .expect("sqlite query")
            .is_none(),
        "T-MEM-4 violation: different project_id must NEVER hit cache in sqlite"
    );
    assert!(
        in_mem
            .cache_lookup(&diff_proj_q)
            .expect("in_mem query")
            .is_none(),
        "T-MEM-4 violation: different project_id must NEVER hit cache in in_mem"
    );

    // 4. Same CWD (/proj/a) with no project_id (outside git) -> NO HIT
    let no_proj_q = CacheQuery {
        intent: "docker run pg",
        os: "macos",
        shell: "zsh",
        cwd: "/proj/a",
        project_id: None,
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite
            .cache_lookup(&no_proj_q)
            .expect("sqlite query")
            .is_none(),
        "T-MEM-4 violation: project vs outside project must NEVER hit cache"
    );
    assert!(
        in_mem
            .cache_lookup(&no_proj_q)
            .expect("in_mem query")
            .is_none(),
        "T-MEM-4 violation: project vs outside project must NEVER hit cache"
    );
}

/// Test cache eligibility rules:
/// - Danger results never cached or returned
/// - Unmarked 1x -> no hit
/// - Unmarked 2x -> hit
/// - Accepted 1x -> hit
/// - Rejected -> no hit
#[test]
fn t_cache_eligibility_rules_and_danger_rejection() {
    let dir = unique_dir("eligibility");
    let sqlite = SqliteStore::open(&dir.join("cache.db")).expect("open");

    // Danger validation function strictly returns Err for Danger
    assert!(validate_cacheable(RiskLevel::Danger).is_err());
    assert!(validate_cacheable(RiskLevel::Review).is_err());
    assert!(validate_cacheable(RiskLevel::Safe).is_ok());

    // 1. Record a Danger interaction into store
    let danger_item = make_interaction(
        1,
        "wipe disk",
        "rm -rf /",
        "/tmp",
        None,
        RiskLevel::Danger,
        Some(true),
    );
    sqlite.record_interaction(&danger_item).expect("record");

    let query_danger = CacheQuery {
        intent: "wipe disk",
        os: "macos",
        shell: "zsh",
        cwd: "/tmp",
        project_id: None,
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite.cache_lookup(&query_danger).expect("query").is_none(),
        "danger command must NEVER be returned as a cache hit"
    );

    // 2. Unmarked once -> no hit
    let run1 = make_interaction(
        2,
        "list files",
        "ls -la",
        "/tmp",
        None,
        RiskLevel::Safe,
        None,
    );
    sqlite.record_interaction(&run1).expect("record");
    let query_list = CacheQuery {
        intent: "list files",
        os: "macos",
        shell: "zsh",
        cwd: "/tmp",
        project_id: None,
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite.cache_lookup(&query_list).expect("query").is_none(),
        "unmarked command executed once must not be cache hit"
    );

    // 3. Unmarked second time -> hits cache! (unmarked but repeated 2+ times)
    let run2 = make_interaction(
        3,
        "  list   files  ", // varied whitespace
        "ls -la",
        "/tmp",
        None,
        RiskLevel::Safe,
        None,
    );
    sqlite.record_interaction(&run2).expect("record");
    let hit = sqlite
        .cache_lookup(&query_list)
        .expect("query")
        .expect("must hit after 2 repetitions");
    assert_eq!(hit.command, "ls -la");
    assert_eq!(hit.confidence, Some(1.0));
    assert_eq!(hit.risk_level, RiskLevel::Safe);

    // 4. Rejected interaction -> no hit
    let run_rej = make_interaction(
        4,
        "show git status",
        "git status --porcelain",
        "/tmp",
        None,
        RiskLevel::Safe,
        Some(false), // rejected
    );
    sqlite.record_interaction(&run_rej).expect("record");
    let query_status = CacheQuery {
        intent: "show git status",
        os: "macos",
        shell: "zsh",
        cwd: "/tmp",
        project_id: None,
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };
    assert!(
        sqlite.cache_lookup(&query_status).expect("query").is_none(),
        "rejected command must never hit cache"
    );
}

/// Latency budget test: cache-hit path must be < 5 ms in a loop.
#[test]
fn t_cache_bench_latency_under_5ms() {
    let dir = unique_dir("bench");
    let sqlite = SqliteStore::open(&dir.join("cache.db")).expect("open");

    let item = make_interaction(
        1,
        "cargo check",
        "cargo check --workspace",
        "/repo",
        Some("bench-proj"),
        RiskLevel::Safe,
        Some(true),
    );
    sqlite.record_interaction(&item).expect("record");

    let query = CacheQuery {
        intent: "cargo check",
        os: "macos",
        shell: "zsh",
        cwd: "/repo",
        project_id: Some("bench-proj"),
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    };

    // Warmup
    assert!(sqlite.cache_lookup(&query).expect("query").is_some());

    // 100 iterations
    let iters = 100;
    let start = Instant::now();
    for _ in 0..iters {
        let hit = sqlite.cache_lookup(&query).expect("query");
        assert!(hit.is_some());
    }
    let total_elapsed = start.elapsed();
    let per_iter = total_elapsed / iters;

    assert!(
        per_iter.as_millis() < 5,
        "cache hit path must be < 5 ms; measured {:?}",
        per_iter
    );
}
