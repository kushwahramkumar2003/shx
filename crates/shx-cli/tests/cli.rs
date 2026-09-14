//! T-CLI-1 / T-CLI-2 / T-CLI-3 and `--json` shape.

use std::fs;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

use assert_cmd::Command;

const FIXTURE: &str =
    "docker run --name pg -e POSTGRES_PASSWORD=postgres -p 7000:5432 -d postgres:16\n";

fn unique_home() -> PathBuf {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("time")
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("shx-cli-{}-{nanos}", std::process::id()));
    fs::create_dir_all(&dir).expect("temp home");
    dir
}

fn shx() -> Command {
    let home = unique_home();
    let mut cmd = Command::cargo_bin("shx").expect("shx bin");
    cmd.env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", &home)
        .current_dir(&home);
    cmd
}

/// T-CLI-1: stdout is the command only — no ANSI, no explanation.
#[test]
fn t_cli_1_stdout_pure() {
    let assert = shx()
        .args(["--offline", "run pg on 7000"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, FIXTURE);
    assert!(
        !stdout.contains('\u{1b}'),
        "stdout must not contain ANSI: {stdout:?}"
    );
    assert!(
        !stdout.to_lowercase().contains("start"),
        "explanation must not be on stdout: {stdout:?}"
    );
    assert!(
        stderr.contains("Postgres") || stderr.contains("postgres") || !stderr.is_empty(),
        "explanation belongs on stderr: {stderr:?}"
    );
}

/// T-CLI-2: warnings go to stderr; stdout stays the command.
#[test]
fn t_cli_2_warnings_on_stderr() {
    let home = unique_home();
    let cfg = home.join("shx.toml");
    fs::write(&cfg, "[backend]\nmode = \"local-first\"\nmystery = true\n").unwrap();
    let assert = shx()
        .args([
            "--config",
            cfg.to_str().unwrap(),
            "--offline",
            "run pg on 7000",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, FIXTURE);
    assert!(
        stderr.contains("warning:") && stderr.contains("mystery"),
        "unknown-key warning must be on stderr: {stderr:?}"
    );
}

/// T-CLI-3: exit codes 0 / 2 / 4 for the paths T-006 can hit.
#[test]
fn t_cli_3_exit_codes() {
    shx().args(["--offline", "run pg on 7000"]).assert().code(0);
    shx().args(["--offline"]).assert().code(2);
    shx()
        .args(["--cloud", "--offline", "run pg on 7000"])
        .assert()
        .code(2);
    shx().args(["--cloud", "run pg on 7000"]).assert().code(2);
    shx().args(["run pg on 7000"]).assert().code(4);
}

#[test]
fn json_shape() {
    let assert = shx()
        .args(["--offline", "--json", "run pg on 7000"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["version"], 1);
    assert_eq!(v["input"], "run pg on 7000");
    assert_eq!(v["commands"][0]["command"], FIXTURE.trim_end());
    assert_eq!(v["risk"]["level"], "safe");
    assert_eq!(v["backend"]["id"], "mock");
    assert_eq!(v["backend"]["model"], "fixture");
    assert!(v["backend"]["escalated_from"].is_null());
    assert_eq!(v["memory"]["used"], false);
    assert_eq!(v["memory"]["from_cache"], false);
    assert_eq!(v["exit_reason"], "ok");
    assert!(v["latency_ms"].is_number());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        !stderr.contains("Starts a detached"),
        "--json must not print explanation on stderr: {stderr:?}"
    );
}

#[test]
fn version_on_stdout() {
    let assert = shx().arg("--version").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("shx"), "{stdout:?}");
    assert!(stdout.contains("0.0.0"), "{stdout:?}");
}
