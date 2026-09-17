//! T-603: `-n` candidates print one command per stdout line; `--copy`
//! keeps stdout pure and degrades gracefully.

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
        "shx-n-{}-{nanos}-{n}-{}",
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

const FIRST: &str =
    "docker run --name pg -e POSTGRES_PASSWORD=postgres -p 7000:5432 -d postgres:16";
const SECOND: &str = "docker run --rm -p 7000:5432 -d postgres:16";
const THIRD: &str = "docker compose run --service-ports db";

/// `-n 3` prints one command per stdout line, ranked best first.
#[test]
fn t_603_n3_prints_three_lines() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args([
            "--offline",
            "-n",
            "3",
            "start postgres in docker on port 7000",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(!stdout.contains('\u{1b}'), "no ANSI on stdout: {stdout:?}");
    assert_eq!(stdout, format!("{FIRST}\n{SECOND}\n{THIRD}\n"));
}

/// `-n` truncates; fewer candidates than requested prints what exists.
#[test]
fn t_603_n_truncates_and_floor_is_available() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args([
            "--offline",
            "-n",
            "2",
            "start postgres in docker on port 7000",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout, format!("{FIRST}\n{SECOND}\n"));

    // No `-n`: a single candidate (default `candidates = 1`).
    let assert = shx_in(&home)
        .args(["--offline", "start postgres in docker on port 7000"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout, format!("{FIRST}\n"));

    // One candidate requested of three: still one line.
    let assert = shx_in(&home)
        .args(["--offline", "run pg on 7000", "-n", "3"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout.lines().count(), 1, "{stdout:?}");
}

/// `--json` carries all candidates in order.
#[test]
fn t_603_n3_json_shape() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args([
            "--offline",
            "--no-memory",
            "--json",
            "-n",
            "3",
            "start postgres in docker on port 7000",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("json");
    let cmds = v["commands"].as_array().expect("commands");
    assert_eq!(cmds.len(), 3);
    assert_eq!(cmds[0]["command"], FIRST);
    assert_eq!(cmds[1]["command"], SECOND);
    assert_eq!(cmds[2]["command"], THIRD);
}

/// `--copy` keeps stdout pure and exits 0; a clipboard failure (headless CI
/// or a feature-off build) degrades to a stderr warning, never stdout.
#[test]
fn t_603_copy_keeps_stdout_pure() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--copy", "run pg on 7000"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stdout.contains("docker run --name pg"),
        "command still printed: {stdout:?}"
    );
    assert!(
        !stdout.contains("warning:"),
        "warnings stay off stdout: {stdout:?}"
    );
    assert!(!stdout.contains('\u{1b}'), "no ANSI on stdout: {stdout:?}");
    if stderr.contains("--copy ignored") {
        assert!(
            !stderr.contains("docker run --name pg"),
            "warning must not echo the command: {stderr:?}"
        );
    }
}

/// `--copy` with `-n 3` still prints all three lines and exits 0.
#[test]
fn t_603_copy_with_n3() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args([
            "--offline",
            "-n",
            "3",
            "--copy",
            "start postgres in docker on port 7000",
        ])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(stdout, format!("{FIRST}\n{SECOND}\n{THIRD}\n"));
}

/// `--copy` also works in `--explain` mode without polluting stdout.
#[test]
fn t_603_copy_in_explain_mode() {
    let home = unique_home();
    let assert = shx_in(&home)
        .args(["--offline", "--copy", "--explain", "ls -la"])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
}
