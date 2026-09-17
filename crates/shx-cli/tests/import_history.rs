//! T-605: `shx import-history` — redaction before insert, summary on
//! stderr, `--dry-run` writes nothing.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;

static HOME_COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_home() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let n = HOME_COUNTER.fetch_add(1, Ordering::SeqCst);
    let tid = format!("{:?}", std::thread::current().id());
    let dir = std::env::temp_dir().join(format!(
        "shx-import-{}-{nanos}-{n}-{}",
        std::process::id(),
        tid.replace(['(', ')', ' ', ','], "_")
    ));
    fs::create_dir_all(&dir).expect("temp home");
    dir
}

fn shx_in(home: &Path) -> Command {
    let mut cmd = Command::cargo_bin("shx").expect("shx bin");
    cmd.env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home)
        .env("LOCALAPPDATA", home)
        .env("XDG_DATA_HOME", home.join(".local/share"))
        .current_dir(home);
    cmd
}

/// shell_history rows as (ts, cmd, source), in insert order.
fn shell_rows(home: &Path) -> Vec<(i64, String, String)> {
    let db = home.join(".local/share/shx/shx.db");
    if !db.exists() {
        return Vec::new();
    }
    let conn = rusqlite::Connection::open(&db).expect("open db");
    let mut stmt = conn
        .prepare("SELECT ts, cmd, source FROM shell_history ORDER BY id")
        .expect("prepare");
    stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))
        .expect("query")
        .map(|r| r.expect("row"))
        .collect()
}

const SECRET: &str = "sk-TESTFAKE0000000000000000";

/// zsh: extended + plain import, secret redacted, malformed skipped, exact
/// summary on stderr, stdout empty.
#[test]
fn t_605_zsh_import_redacts_and_summarizes() {
    let home = unique_home();
    let hist = home.join("hist.zsh");
    fs::write(
        &hist,
        format!(": 1700000000:0;git status\nexport TOKEN={SECRET}\n: nonsense\n\nls -la\n"),
    )
    .unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "zsh",
            "--file",
            hist.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(
        stderr.trim(),
        "imported 3, redacted 1, skipped 1 malformed",
        "{stderr:?}"
    );

    let rows = shell_rows(&home);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0], (1700000000, "git status".into(), "zsh".into()));
    assert!(
        !rows[1].1.contains(SECRET),
        "raw secret must not land on disk: {}",
        rows[1].1
    );
    assert!(
        rows[1].1.contains("«redacted:"),
        "expected redaction marker: {}",
        rows[1].1
    );
    assert_eq!(rows[2].1, "ls -la");
    assert!(rows.iter().all(|r| r.2 == "zsh"));
}

/// bash: `#<epoch>` timestamps attach to the next command; `#`-comments are
/// ordinary commands; a trailing timestamp is dropped, not counted.
#[test]
fn t_605_bash_timestamps_and_comments() {
    let home = unique_home();
    let hist = home.join("hist.bash");
    fs::write(
        &hist,
        "#1700000000\ngit status\n# deploy to prod\nls\n#1700000001\n",
    )
    .unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "bash",
            "--file",
            hist.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stderr.trim(), "imported 3, redacted 0, skipped 0 malformed");

    let rows = shell_rows(&home);
    assert_eq!(
        rows,
        vec![
            (1700000000, "git status".into(), "bash".into()),
            (0, "# deploy to prod".into(), "bash".into()),
            (0, "ls".into(), "bash".into()),
        ]
    );
}

/// fish: stanzas with and without `when:`, quoted commands, bad lines skipped.
#[test]
fn t_605_fish_stanzas() {
    let home = unique_home();
    let hist = home.join("history.fish");
    fs::write(
        &hist,
        "- cmd: git status\n  when: 1700000000\n- cmd: ls\n- cmd: \"echo hi\"\n  when: nope\njunk line\n",
    )
    .unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "fish",
            "--file",
            hist.to_str().unwrap(),
        ])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stderr.trim(), "imported 3, redacted 0, skipped 2 malformed");

    let rows = shell_rows(&home);
    assert_eq!(
        rows,
        vec![
            (1700000000, "git status".into(), "fish".into()),
            (0, "ls".into(), "fish".into()),
            (0, "echo hi".into(), "fish".into()),
        ]
    );
}

/// `--dry-run` reports prospects and writes nothing (no DB file at all).
#[test]
fn t_605_dry_run_writes_nothing() {
    let home = unique_home();
    let hist = home.join("hist.zsh");
    fs::write(
        &hist,
        format!(": 1700000000:0;git status\nexport TOKEN={SECRET}\n"),
    )
    .unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "zsh",
            "--file",
            hist.to_str().unwrap(),
            "--dry-run",
        ])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("dry-run") && stderr.contains("would import 2"),
        "{stderr:?}"
    );
    assert!(
        !home.join(".local/share/shx/shx.db").exists(),
        "dry-run must not create the database"
    );
}

/// `--limit` keeps the most recent N entries.
#[test]
fn t_605_limit_takes_most_recent() {
    let home = unique_home();
    let hist = home.join("hist.bash");
    fs::write(&hist, "one\ntwo\nthree\nfour\nfive\n").unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "bash",
            "--file",
            hist.to_str().unwrap(),
            "--limit",
            "2",
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stderr.trim(), "imported 2, redacted 0, skipped 0 malformed");
    let rows = shell_rows(&home);
    assert_eq!(
        rows.iter().map(|r| r.1.as_str()).collect::<Vec<_>>(),
        vec!["four", "five"]
    );
}

/// `--limit 0` is a successful no-op.
#[test]
fn t_605_limit_zero_imports_nothing() {
    let home = unique_home();
    let hist = home.join("hist.bash");
    fs::write(&hist, "one\ntwo\n").unwrap();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "bash",
            "--file",
            hist.to_str().unwrap(),
            "--limit",
            "0",
        ])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stderr.trim(), "imported 0, redacted 0, skipped 0 malformed");
    assert!(shell_rows(&home).is_empty());
}

/// Unknown `--shell` is a usage error with empty stdout.
#[test]
fn t_605_unknown_shell_usage() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["import-history", "--shell", "powershell"])
        .assert()
        .code(2);
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("zsh") && stderr.contains("bash") && stderr.contains("fish"),
        "{stderr:?}"
    );
}

/// Missing file is an error (exit 1) with empty stdout.
#[test]
fn t_605_missing_file_error() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args([
            "import-history",
            "--shell",
            "bash",
            "--file",
            home.join("nope").to_str().unwrap(),
        ])
        .assert()
        .code(1);
    assert!(assert.get_output().stdout.is_empty());
}

/// `--shell` defaults to `$SHELL` when omitted.
#[test]
fn t_605_infers_shell_from_env() {
    let home = unique_home();
    let hist = home.join("hist.zsh");
    fs::write(&hist, "ls\n").unwrap();
    shx_in(&home)
        .env("SHELL", "/bin/zsh")
        .args(["import-history", "--file", hist.to_str().unwrap()])
        .assert()
        .success();
    assert_eq!(shell_rows(&home).len(), 1);

    // Uninferrable shells are a usage error.
    let home2 = unique_home();
    let hist2 = home2.join("hist");
    fs::write(&hist2, "ls\n").unwrap();
    let assert = shx_in(&home2)
        .env("SHELL", "/bin/pwsh")
        .args(["import-history", "--file", hist2.to_str().unwrap()])
        .assert()
        .code(2);
    assert!(assert.get_output().stdout.is_empty());
}

/// No `--file`: reads the shell's default history path under HOME.
#[test]
fn t_605_default_file_per_shell() {
    let home = unique_home();
    fs::write(home.join(".bash_history"), "git status\n").unwrap();
    let assert = shx_in(&home)
        .args(["import-history", "--shell", "bash"])
        .assert()
        .success();
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stderr.trim(), "imported 1, redacted 0, skipped 0 malformed");
    assert_eq!(shell_rows(&home).len(), 1);
}
