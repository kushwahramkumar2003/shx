//! T-601: `shx --explain` reverse mode — prose on stderr, stdout empty
//! except with `--json`.

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
        "shx-explain-{}-{nanos}-{n}-{}",
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

/// Prose goes to stderr; stdout stays empty; exit 0.
#[test]
fn t_601_explain_prose_on_stderr_stdout_empty() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--explain", "ls -la"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stdout.is_empty(),
        "stdout must stay empty in explain mode: {stdout:?}"
    );
    assert!(!stdout.contains('\u{1b}'), "no ANSI on stdout: {stdout:?}");
    assert!(
        !stderr.trim().is_empty(),
        "prose belongs on stderr: {stderr:?}"
    );
}

/// A dangerous command is still explained (never refused) with a banner.
#[test]
fn t_601_explain_danger_banner_on_stderr() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--explain", "rm -rf /"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stdout.is_empty(),
        "stdout must stay empty for danger too: {stdout:?}"
    );
    assert!(
        stderr.contains("DANGER:") && stderr.contains("why:"),
        "danger banner belongs on stderr: {stderr:?}"
    );
}

/// `--exit-on-risk` turns a risky explanation into exit 3 (stdout empty).
#[test]
fn t_601_explain_exit_on_risk() {
    let home = unique_home();
    // NB: `--explain` consumes the next arg as its value, so it comes last.
    let assert = shx_in(&home)
        .args(["--offline", "--exit-on-risk", "--explain", "rm -rf /"])
        .assert()
        .code(3);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.is_empty(), "stdout empty on exit 3: {stdout:?}");

    shx_in(&home)
        .args(["--offline", "--exit-on-risk", "--explain", "ls -la"])
        .assert()
        .success();
}

/// `--json` puts the versioned object on stdout; the command echoed is the
/// user's own input verbatim, never the model echo.
#[test]
fn t_601_explain_json_shape_and_verbatim_command() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--json", "--explain", "rm -rf /"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["version"], 1);
    assert_eq!(v["input"], "rm -rf /");
    assert_eq!(v["commands"][0]["command"], "rm -rf /");
    assert!(
        v["commands"][0]["explanation"]
            .as_str()
            .is_some_and(|s| !s.is_empty()),
        "prose travels in explanation: {v}"
    );
    assert_eq!(v["risk"]["level"], "danger");
    assert_eq!(v["backend"]["id"], "mock");
    assert_eq!(v["exit_reason"], "ok");
    assert!(v["latency_ms"].is_number());
    assert!(
        stderr.contains("DANGER:"),
        "banner still on stderr in --json: {stderr:?}"
    );
}

/// Usage errors exit 2 with empty stdout.
#[test]
fn t_601_explain_usage_errors() {
    let home = unique_home();
    // Empty command.
    let assert = shx_in(&home)
        .args(["--offline", "--explain", ""])
        .assert()
        .code(2);
    assert!(
        assert.get_output().stdout.is_empty(),
        "stdout empty on usage error"
    );
    // Trailing intent words are rejected.
    let assert = shx_in(&home)
        .args(["--offline", "--explain", "ls", "extra-words"])
        .assert()
        .code(2);
    assert!(
        assert.get_output().stdout.is_empty(),
        "stdout empty on usage error"
    );
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("--explain"), "{stderr:?}");
}

/// Flag conflicts mirror translate mode: `--cloud` + `--offline` exits 2,
/// unconfigured `--cloud` exits 2, all with empty stdout.
#[test]
fn t_601_explain_flag_conflicts() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--cloud", "--offline", "--explain", "ls"])
        .assert()
        .code(2);
    assert!(assert.get_output().stdout.is_empty());

    let assert = shx_in(&home)
        .args(["--cloud", "--explain", "ls"])
        .assert()
        .code(2);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.contains("--cloud unconfigured"), "{stderr:?}");

    // `--local --offline` works like translate mode.
    let assert = shx_in(&home)
        .args(["--local", "--offline", "--explain", "ls -la"])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
}

/// Explaining records nothing: history stays empty afterwards.
#[test]
fn t_601_explain_records_no_memory() {
    let home = unique_home();
    shx_in(&home)
        .args(["--offline", "--explain", "ls -la"])
        .assert()
        .success();
    let assert = shx_in(&home).args(["history", "--json"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(
        v.as_array().map(Vec::len).unwrap_or(99),
        0,
        "explain must not record translations: {v}"
    );
}

/// `--why` routing trace stays on stderr, never stdout.
#[test]
fn t_601_explain_why_on_stderr() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--why", "--explain", "ls -la"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.contains("why:"), "{stderr}");
    assert!(stderr.contains("routing:"), "{stderr}");
}

/// `-q` suppresses prose but keeps the risk banner.
#[test]
fn t_601_explain_quiet_keeps_banner() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "-q", "--explain", "rm -rf /"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stdout.is_empty(), "{stdout:?}");
    assert!(stderr.contains("DANGER:"), "{stderr:?}");
    assert!(
        !stderr.contains("deterministic fallback"),
        "quiet suppresses prose: {stderr:?}"
    );
}
