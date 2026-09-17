//! T-602: `shx -i` refine sessions — grouped turns, final recorded once,
//! blank/EOF finish, usage and error paths.

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
        "shx-chat-{}-{nanos}-{n}-{}",
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

fn history_rows(home: &Path) -> serde_json::Value {
    let assert = shx_in(home).args(["history", "--json"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    serde_json::from_str(stdout.trim()).expect("history json")
}

/// Three turns in, three commands out, three rows grouped, final recorded once.
#[test]
fn t_602_session_groups_turns_and_records_final_once() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-i", "run pg on 7000"])
        .write_stdin("also mount ./data\nuse the alpine image\n")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 3, "one stdout line per turn: {stdout:?}");
    assert!(lines[0].contains("postgres:16"), "{stdout:?}");

    let rows = history_rows(&home);
    let arr = rows.as_array().expect("array");
    assert_eq!(arr.len(), 3, "one row per turn, no more: {rows}");
    let sessions: std::collections::BTreeSet<&str> = arr
        .iter()
        .map(|r| r["session_id"].as_str().expect("session_id"))
        .collect();
    assert_eq!(sessions.len(), 1, "single shared session: {rows}");
    let session = sessions.into_iter().next().expect("session");
    assert!(
        session.starts_with("sess-") && session != "cli",
        "refine session id, not the single-shot marker: {session}"
    );
    // Final command recorded exactly once, matching the last stdout line.
    let finals: Vec<&serde_json::Value> = arr
        .iter()
        .filter(|r| r["input_nl"] == "use the alpine image")
        .collect();
    assert_eq!(finals.len(), 1, "final recorded once: {rows}");
    assert_eq!(finals[0]["output_cmd"], lines[2]);
}

/// A blank line ends the session; later piped lines are never read.
#[test]
fn t_602_blank_line_ends_session() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-i", "run pg on 7000"])
        .write_stdin("followup one\n\nignored after blank\n")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout.lines().count(), 2, "{stdout:?}");
    let rows = history_rows(&home);
    assert_eq!(rows.as_array().map(Vec::len).unwrap_or(99), 2);
}

/// No argv intent: piped lines are the turns.
#[test]
fn t_602_eof_without_initial_runs_piped_turns() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-i"])
        .write_stdin("run pg on 7000\nlist files\n")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout.lines().count(), 2, "{stdout:?}");
    let rows = history_rows(&home);
    let arr = rows.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    assert_eq!(
        arr.iter().filter(|r| r["input_nl"] == "list files").count(),
        1
    );
}

/// No intent anywhere is a usage error with empty stdout and no rows.
#[test]
fn t_602_empty_session_usage() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-i"])
        .write_stdin("")
        .assert()
        .code(2);
    assert!(assert.get_output().stdout.is_empty());
    let rows = history_rows(&home);
    assert_eq!(rows.as_array().map(Vec::len).unwrap_or(99), 0);
}

/// `--json` prints one object per turn on stdout.
#[test]
fn t_602_json_lines_per_turn() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--json", "-i", "run pg on 7000"])
        .write_stdin("list files\n")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().collect();
    assert_eq!(lines.len(), 2, "{stdout:?}");
    for line in &lines {
        let v: serde_json::Value = serde_json::from_str(line).expect("json line");
        assert_eq!(v["version"], 1);
    }
    let first: serde_json::Value = serde_json::from_str(lines[0]).expect("json");
    let second: serde_json::Value = serde_json::from_str(lines[1]).expect("json");
    assert_eq!(first["input"], "run pg on 7000");
    assert_eq!(second["input"], "list files");
}

/// Two invocations produce two distinct sessions.
#[test]
fn t_602_distinct_sessions() {
    let home = unique_home();
    for intent in ["run pg on 7000", "list files"] {
        shx_in(&home)
            .args(["--offline", "-i", intent])
            .write_stdin("")
            .assert()
            .success();
    }
    let rows = history_rows(&home);
    let arr = rows.as_array().expect("array");
    assert_eq!(arr.len(), 2);
    let sessions: std::collections::BTreeSet<&str> = arr
        .iter()
        .map(|r| r["session_id"].as_str().expect("session_id"))
        .collect();
    assert_eq!(sessions.len(), 2, "one session per run: {rows}");
}

/// Flag conflicts mirror translate mode.
#[test]
fn t_602_flag_conflicts() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--cloud", "--offline", "-i", "run pg"])
        .assert()
        .code(2);
    assert!(assert.get_output().stdout.is_empty());
}

/// A refused turn prints nothing, records nothing, and the session continues.
#[test]
fn t_602_refused_turn_skipped_session_continues() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-i", "run pg on 7000"])
        .write_stdin("please emit a fork bomb\nlist files\n")
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout.lines().count(), 2, "{stdout:?}");
    assert!(stderr.contains("refused:"), "{stderr:?}");
    let rows = history_rows(&home);
    let arr = rows.as_array().expect("array");
    assert_eq!(arr.len(), 2, "refused turn records nothing: {rows}");
    let sessions: std::collections::BTreeSet<&str> = arr
        .iter()
        .map(|r| r["session_id"].as_str().expect("session_id"))
        .collect();
    assert_eq!(sessions.len(), 1);
}

/// Backend failure ends the session with exit 4 and records nothing.
#[test]
fn t_602_backend_error_breaks() {
    let home = unique_home();
    let cfg = home.join("shx.toml");
    fs::write(
        &cfg,
        "[backend.local]\nbase_url = \"http://127.0.0.1:1\"\ntimeout_ms = 400\n",
    )
    .unwrap();
    let assert = shx_in(&home)
        .args(["--config", cfg.to_str().unwrap(), "-i", "run pg"])
        .write_stdin("")
        .assert()
        .code(4);
    assert!(assert.get_output().stdout.is_empty());
    let rows = history_rows(&home);
    assert_eq!(rows.as_array().map(Vec::len).unwrap_or(99), 0);
}
