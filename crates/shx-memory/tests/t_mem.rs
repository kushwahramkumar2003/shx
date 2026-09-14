//! T-MEM-3 and Unix permission checks.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::Connection;
use shx_core::{Interaction, RiskLevel, Scope};
use shx_memory::{InMemoryStore, MemoryStore, SqliteStore, record};

fn unique_dir() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("shx-mem-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// T-MEM-3: open a v0 fixture, migrate to v1, rows survive.
#[test]
fn t_mem_3_v0_fixture_migrates_and_rows_survive() {
    let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/v0.db");
    let dir = unique_dir();
    let dest = dir.join("v0.db");
    fs::copy(&src, &dest).expect("copy fixture");

    let before = {
        let c = Connection::open(&dest).unwrap();
        let v: i32 = c
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(v, 0, "fixture must be user_version=0");
        let n: i64 = c
            .query_row("SELECT count(*) FROM interactions", [], |r| r.get(0))
            .unwrap();
        n
    };
    assert!(before >= 1, "fixture should contain rows");

    let store = SqliteStore::open(&dest).expect("open+migrate");
    let rows = store.recent(10, Scope::Tool).expect("recent");
    assert_eq!(rows.len() as i64, before);

    let v: i32 = {
        let c = Connection::open(&dest).unwrap();
        c.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    };
    assert_eq!(v, 1);
}

/// T-MEM-1: a fake key is masked on write and never comes back raw.
#[test]
fn t_mem_1_record_fake_key_read_back_masked() {
    let secret = "sk-TESTFAKE0000000000000000";
    let raw = Interaction {
        id: None,
        ts: 1,
        session_id: "s".into(),
        project_id: None,
        cwd: "/tmp".into(),
        os: "macos".into(),
        shell: "zsh".into(),
        input_nl: format!("export TOKEN={secret}"),
        output_cmd: format!("curl -H token={secret} https://api"),
        explanation: Some(format!("uses {secret}")),
        backend: "mock".into(),
        model: "fixture".into(),
        confidence: Some(0.8),
        latency_ms: 42,
        risk_level: RiskLevel::Review,
        risk_notes: vec!["secrets.inline".into()],
        from_cache: false,
        accepted: None,
        executed: None,
        tags: vec![],
    };
    for store in [
        &InMemoryStore::default() as &dyn MemoryStore,
        &SqliteStore::open_in_memory().unwrap() as &dyn MemoryStore,
    ] {
        let id = store.record_interaction(&raw).unwrap();
        let rows = store.recent(1, Scope::Tool).unwrap();
        assert_eq!(rows[0].id, Some(id));
        assert!(!rows[0].from_cache);
        assert_eq!(rows[0].risk_level, RiskLevel::Review);
        assert_eq!(rows[0].latency_ms, 42);
        for field in [
            &rows[0].input_nl,
            &rows[0].output_cmd,
            rows[0].explanation.as_deref().unwrap_or(""),
        ] {
            assert!(
                !record::looks_unredacted_secret(field),
                "raw secret survived: {field}"
            );
            assert!(
                field.contains("«redacted:"),
                "expected redaction marker in {field}"
            );
        }
    }
}

#[cfg(unix)]
#[test]
fn unix_perms_dir_0700_file_0600() {
    use std::os::unix::fs::PermissionsExt;
    let dir = unique_dir().join("nested");
    let path = dir.join("shx.db");
    let _store = SqliteStore::open(&path).expect("create");
    let dir_mode = fs::metadata(&dir).unwrap().permissions().mode() & 0o777;
    let file_mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(dir_mode, 0o700, "dir {dir_mode:o}");
    assert_eq!(file_mode, 0o600, "file {file_mode:o}");
}
