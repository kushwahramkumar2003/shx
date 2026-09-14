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
        .env("LOCALAPPDATA", &home)
        .current_dir(&home);
    cmd
}

fn shx_in(home: &std::path::Path) -> Command {
    let mut cmd = Command::cargo_bin("shx").expect("shx bin");
    cmd.env_clear()
        .env("HOME", home)
        .env("USERPROFILE", home)
        .env("APPDATA", home)
        .env("LOCALAPPDATA", home)
        .current_dir(home);
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

/// T-CLI-2: a Danger banner stays on stderr; stdout is the command only.
#[test]
fn t_cli_2_risk_banner_on_stderr() {
    let assert = shx()
        .args(["--offline", "--no-memory", "wipe the root filesystem"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, "rm -rf /\n");
    assert!(
        !stdout.to_uppercase().contains("DANGER"),
        "banner must not be on stdout: {stdout:?}"
    );
    assert!(
        stderr.contains("DANGER:") && stderr.contains("why:"),
        "danger banner belongs on stderr: {stderr:?}"
    );
}

#[test]
fn quiet_keeps_risk_banner() {
    let assert = shx()
        .args(["--offline", "--no-memory", "-q", "wipe the root filesystem"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, "rm -rf /\n");
    assert!(
        stderr.contains("DANGER:"),
        "quiet keeps banners: {stderr:?}"
    );
    assert!(
        !stderr.contains("Recursively force-deletes"),
        "quiet suppresses explanation: {stderr:?}"
    );
}

#[test]
fn refuse_prints_nothing_on_stdout() {
    let assert = shx()
        .args([
            "--offline",
            "--no-memory",
            "delete my entire home directory and all backups",
        ])
        .assert()
        .code(6);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stdout.is_empty(),
        "refuse must not print a command: {stdout:?}"
    );
    assert!(
        stderr.contains("refused:"),
        "refuse reason on stderr: {stderr:?}"
    );
}

#[test]
fn refuse_json_exit_reason() {
    let assert = shx()
        .args([
            "--offline",
            "--no-memory",
            "--json",
            "delete my entire home directory and all backups",
        ])
        .assert()
        .code(6);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["exit_reason"], "refused");
    assert!(v["commands"].as_array().is_some_and(|a| a.is_empty()));
}

#[test]
fn exit_on_risk_json_reason() {
    let assert = shx()
        .args([
            "--offline",
            "--no-memory",
            "--json",
            "--exit-on-risk",
            "kill all processes",
        ])
        .assert()
        .code(3);
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["exit_reason"], "risk");
    assert_eq!(v["risk"]["level"], "review");
    assert_eq!(v["commands"][0]["command"], "kill -9 -1");
}

#[test]
fn refuse_multi_command_on_risk_keeps_first() {
    let assert = shx()
        .args([
            "--offline",
            "--no-memory",
            "reset git then delete everything",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, "git reset --hard\n");
    assert!(
        !stdout.contains("rm -rf"),
        "chained tail must not reach stdout: {stdout:?}"
    );
    assert!(
        stderr.contains("dropped chained commands") && stderr.contains("danger"),
        "warn about dropped chain: {stderr:?}"
    );
}

/// T-CLI-3: exit codes 0 / 2 / 3 / 4 / 6 per docs/05-CLI-SPEC.md §5.
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
    shx()
        .args([
            "--offline",
            "--no-memory",
            "--exit-on-risk",
            "kill all processes",
        ])
        .assert()
        .code(3);
    shx()
        .args([
            "--offline",
            "--no-memory",
            "delete my entire home directory and all backups",
        ])
        .assert()
        .code(6);
}

#[test]
fn json_shape() {
    let assert = shx()
        .args(["--offline", "--no-memory", "--json", "run pg on 7000"])
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
fn doctor_json_reports_checks() {
    let assert = shx()
        .args(["--offline", "doctor", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["ok"], true);
    assert_eq!(v["checks"]["config_parse"]["ok"], true);
    assert_eq!(v["checks"]["db_path"]["stub"], true);
    assert!(v["checks"]["db_path"]["path"].as_str().is_some());
    assert_eq!(v["checks"]["backend"]["ok"], true);
    assert_eq!(v["checks"]["backend"]["id"], "mock");
    assert_eq!(v["checks"]["backend"]["reachable"], true);
    assert!(v["config"].is_object(), "effective config: {v}");
    assert!(v["config"]["backend"].is_object());
}

#[test]
fn doctor_unreachable_exits_4() {
    let home = unique_home();
    let cfg = home.join("shx.toml");
    fs::write(
        &cfg,
        r#"
[backend.local]
base_url = "http://127.0.0.1:1"
timeout_ms = 400
"#,
    )
    .unwrap();
    shx()
        .args(["--config", cfg.to_str().unwrap(), "doctor", "--json"])
        .assert()
        .code(4);
}

#[test]
fn config_init_writes_valid_file() {
    let home = unique_home();
    let dest = home.join("shx.toml");
    shx()
        .args(["--config", dest.to_str().unwrap(), "config", "init"])
        .assert()
        .success();
    let text = fs::read_to_string(&dest).expect("written");
    assert!(text.contains("mode = \"local-first\""));
    let assert = shx()
        .args([
            "--offline",
            "--config",
            dest.to_str().unwrap(),
            "doctor",
            "--json",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v["checks"]["config_parse"]["ok"], true);
}

#[test]
fn config_path_prints_precedence_order() {
    let home = unique_home();
    fs::write(home.join(".shx.toml"), "[ui]\ncandidates = 1\n").unwrap();
    let mut cmd = Command::cargo_bin("shx").expect("shx bin");
    let assert = cmd
        .env_clear()
        .env("HOME", &home)
        .env("USERPROFILE", &home)
        .env("APPDATA", &home)
        .current_dir(&home)
        .args(["config", "path"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let lines: Vec<&str> = stdout.lines().filter(|l| !l.is_empty()).collect();
    assert!(
        lines.iter().any(|l| l.contains("shx.toml")),
        "global path: {stdout:?}"
    );
    assert!(
        lines.iter().any(|l| l.ends_with(".shx.toml")),
        "project path: {stdout:?}"
    );
    let global_idx = lines.iter().position(|l| l.contains("shx.toml")).unwrap();
    let project_idx = lines.iter().position(|l| l.ends_with(".shx.toml")).unwrap();
    assert!(
        global_idx <= project_idx,
        "global before project: {lines:?}"
    );
}

#[test]
fn version_on_stdout() {
    let assert = shx().arg("--version").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("shx"), "{stdout:?}");
    assert!(stdout.contains("0.0.0"), "{stdout:?}");
}

#[test]
fn history_list_show_export_jsonl_roundtrip_purge() {
    let home = unique_home();
    shx_in(&home)
        .args(["--offline", "run pg on 7000"])
        .assert()
        .success();
    shx_in(&home)
        .args(["--offline", "list files"])
        .assert()
        .success();

    let assert = shx_in(&home).args(["history", "--json"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json array");
    let arr = v.as_array().expect("array");
    assert!(arr.len() >= 2, "{stdout}");
    let id = arr[0]["id"].as_i64().expect("id");
    assert!(arr.iter().all(|r| r.get("from_cache").is_some()));
    assert!(arr.iter().all(|r| r.get("risk_level").is_some()));
    assert!(arr.iter().all(|r| r.get("latency_ms").is_some()));

    shx_in(&home)
        .args(["history", "show", &id.to_string(), "--json"])
        .assert()
        .success();

    let out = home.join("export.jsonl");
    shx_in(&home)
        .args([
            "history",
            "export",
            "--jsonl",
            "--out",
            out.to_str().unwrap(),
        ])
        .assert()
        .success();
    let jsonl = fs::read_to_string(&out).expect("jsonl");
    let mut n = 0;
    for line in jsonl.lines().filter(|l| !l.is_empty()) {
        let row: serde_json::Value = serde_json::from_str(line).expect(line);
        assert!(row.get("input_nl").is_some());
        n += 1;
    }
    assert!(n >= 2, "jsonl rows {n}");

    shx_in(&home)
        .args(["history", "purge", "--all"])
        .assert()
        .code(2);
    shx_in(&home)
        .args(["-y", "history", "purge", "--all"])
        .assert()
        .success();
    let assert = shx_in(&home).args(["history", "--json"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v.as_array().map(Vec::len).unwrap_or(99), 0);
}

#[test]
fn history_grep_and_prune() {
    let home = unique_home();
    shx_in(&home)
        .args(["--offline", "run pg on 7000"])
        .assert()
        .success();
    let assert = shx_in(&home)
        .args(["history", "--grep", "pg", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert!(!v.as_array().unwrap().is_empty(), "{stdout}");
    shx_in(&home)
        .args(["history", "prune", "--older-than", "1d"])
        .assert()
        .success();
}

#[test]
fn teach_list_forget() {
    let home = unique_home();
    shx_in(&home)
        .args(["teach", "pg", "postgres"])
        .assert()
        .success();
    let assert = shx_in(&home)
        .args(["teach", "--list", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v[0]["term"], "pg");
    assert_eq!(v[0]["expansion"], "postgres");
    assert_eq!(v[0]["source"], "taught");
    shx_in(&home)
        .args(["teach", "--forget", "pg"])
        .assert()
        .success();
    let assert = shx_in(&home)
        .args(["teach", "--list", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    assert_eq!(v.as_array().map(Vec::len).unwrap_or(1), 0);
}

#[test]
fn why_never_on_stdout() {
    let assert = shx()
        .args(["--offline", "--no-memory", "--why", "run pg on 7000"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert_eq!(stdout, FIXTURE);
    assert!(!stdout.contains("why:"), "why leaked to stdout: {stdout:?}");
    assert!(stderr.contains("why:"), "{stderr}");
    assert!(stderr.contains("placeholder; T-403"), "{stderr}");
    assert!(stderr.contains("memory: used=false"), "{stderr}");
    assert!(stderr.contains("routing: local-first"), "{stderr}");
}
