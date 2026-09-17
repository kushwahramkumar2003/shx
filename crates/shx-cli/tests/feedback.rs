//! T-503: `shx feedback` end-to-end (good/bad, cap/floor, flags, usage).

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
        "shx-fb-{}-{nanos}-{n}-{}",
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

fn history_json(home: &Path) -> serde_json::Value {
    let assert = shx_in(home).args(["history", "--json"]).assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    serde_json::from_str(stdout.trim()).expect("history json")
}

fn find_id(home: &Path, input: &str) -> i64 {
    let v = history_json(home);
    let arr = v.as_array().expect("array");
    arr.iter()
        .find(|r| r["input_nl"] == input)
        .unwrap_or_else(|| panic!("no history for {input:?}: {v}"))["id"]
        .as_i64()
        .expect("id")
}

fn vocab_weight(home: &Path, term: &str, expansion: &str) -> Option<f64> {
    let assert = shx_in(home)
        .args(["teach", "--list", "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let v: serde_json::Value = serde_json::from_str(stdout.trim()).expect("vocab json");
    v.as_array()
        .unwrap()
        .iter()
        .find(|e| e["term"] == term && e["expansion"] == expansion)
        .and_then(|e| e["weight"].as_f64())
}

fn history_show(home: &Path, id: i64) -> serde_json::Value {
    let assert = shx_in(home)
        .args(["history", "show", &id.to_string(), "--json"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    serde_json::from_str(stdout.trim()).expect("show json")
}

/// Good raises to cap 3.0; bad lowers to floor 0.0; stdout stays empty.
#[test]
fn t_503_feedback_good_bad_weight_cap_and_floor() {
    let home = unique_home();
    shx_in(&home)
        .args(["teach", "pg", "postgres"])
        .assert()
        .success();
    shx_in(&home).args(["--offline", "pg"]).assert().success();
    let id = find_id(&home, "pg");

    for (want, label) in [(2.5, "first good"), (3.0, "cap"), (3.0, "over-cap")] {
        let assert = shx_in(&home)
            .args(["feedback", &id.to_string(), "good"])
            .assert()
            .success();
        assert!(
            assert.get_output().stdout.is_empty(),
            "stdout must stay empty ({label})"
        );
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(stderr.contains("feedback"), "{stderr:?}");
        let w = vocab_weight(&home, "pg", "postgres").expect("pg row");
        assert!((w - want).abs() < 1e-9, "{label}: got {w}, want {want}");
    }

    // Bad lowers; drive to the floor and never below.
    let assert = shx_in(&home)
        .args(["feedback", &id.to_string(), "bad"])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    assert!((vocab_weight(&home, "pg", "postgres").unwrap() - 2.5).abs() < 1e-9);
    for _ in 0..10 {
        shx_in(&home)
            .args(["feedback", &id.to_string(), "bad"])
            .assert()
            .success();
    }
    let floored = vocab_weight(&home, "pg", "postgres").unwrap();
    assert!((floored - 0.0).abs() < 1e-9, "floored, got {floored}");
}

/// Bad never creates vocabulary; good creates learned rows needing 3 to apply.
#[test]
fn t_503_feedback_bad_never_creates_good_learns() {
    let home = unique_home();
    shx_in(&home)
        .args(["--offline", "zzbrandnewterm"])
        .assert()
        .success();
    let id = find_id(&home, "zzbrandnewterm");
    shx_in(&home)
        .args(["feedback", &id.to_string(), "bad"])
        .assert()
        .success();
    assert!(
        vocab_weight(&home, "zzbrandnewterm", "zzbrandnewterm").is_none(),
        "bad must not create vocabulary"
    );

    shx_in(&home).args(["--offline", "k8s"]).assert().success();
    let kid = find_id(&home, "k8s");
    shx_in(&home)
        .args(["feedback", &kid.to_string(), "good"])
        .assert()
        .success();
    let w1 = vocab_weight(&home, "k8s", "k8s").expect("learned row");
    assert!((w1 - 0.5).abs() < 1e-9, "one-off starts at 0.5, got {w1}");
}

/// --executed sets executed=1; --accepted forces accepted=1; flag-only works.
#[test]
fn t_503_feedback_executed_and_accepted_flags() {
    let home = unique_home();
    shx_in(&home).args(["--offline", "pg"]).assert().success();
    let id = find_id(&home, "pg");

    let assert = shx_in(&home)
        .args(["feedback", &id.to_string(), "good", "--executed"])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    let row = history_show(&home, id);
    assert_eq!(row["accepted"], true);
    assert_eq!(row["executed"], true);

    // Bad + --accepted forces accepted=1 (override for wrappers).
    shx_in(&home)
        .args(["feedback", &id.to_string(), "bad", "--accepted"])
        .assert()
        .success();
    let row = history_show(&home, id);
    assert_eq!(row["accepted"], true);
    assert_eq!(row["executed"], true, "executed preserved");

    // Flag-only with no verdict leaves accepted alone but sets executed.
    shx_in(&home)
        .args(["--offline", "flagonly"])
        .assert()
        .success();
    let fid = find_id(&home, "flagonly");
    let before = history_show(&home, fid);
    assert!(before.get("accepted").is_some());
    shx_in(&home)
        .args(["feedback", &fid.to_string(), "--executed"])
        .assert()
        .success();
    let after = history_show(&home, fid);
    assert_eq!(after["executed"], true);
}

/// Usage errors exit 2 with empty stdout; unknown ids exit 1.
#[test]
fn t_503_feedback_usage_and_unknown_id() {
    let home = unique_home();

    for args in [
        vec!["feedback"],
        vec!["feedback", "1", "great"],
        vec!["feedback", "1"],
        vec!["feedback", "1", "--note", "hi"],
    ] {
        let assert = shx_in(&home).args(&args).assert().code(2);
        assert!(
            assert.get_output().stdout.is_empty(),
            "stdout empty on usage: {args:?}"
        );
    }

    let assert = shx_in(&home)
        .args(["feedback", "9999", "good"])
        .assert()
        .code(1);
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stderr.contains("no interaction 9999"), "{stderr:?}");

    // Verdict is case-insensitive.
    shx_in(&home).args(["--offline", "pg"]).assert().success();
    let id = find_id(&home, "pg");
    shx_in(&home)
        .args(["feedback", &id.to_string(), "GOOD"])
        .assert()
        .success();
    assert_eq!(history_show(&home, id)["accepted"], true);
}

/// Notes (even secret-looking ones) are accepted; stdout stays pure.
#[test]
fn t_503_feedback_note_and_stdout_pure() {
    let home = unique_home();
    shx_in(&home).args(["--offline", "pg"]).assert().success();
    let id = find_id(&home, "pg");
    let assert = shx_in(&home)
        .args([
            "feedback",
            &id.to_string(),
            "good",
            "--note",
            "token sk-TESTFAKE0000000000000000 leaked?",
        ])
        .assert()
        .success();
    assert!(assert.get_output().stdout.is_empty());
    assert_eq!(history_show(&home, id)["accepted"], true);
}
