//! T-604: `shx completion <shell>` validity per shell, `shx man` roff, and
//! drift checks against the committed archive snapshots.

use std::fs;
use std::path::{Path, PathBuf};

use assert_cmd::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn shx() -> Command {
    let mut cmd = Command::cargo_bin("shx").expect("shx bin");
    cmd.env_clear()
        .env("PATH", std::env::var_os("PATH").unwrap_or_default());
    cmd
}

/// Runtime output is valid per shell: non-empty, shell-specific markers,
/// stderr silent.
#[test]
fn t_604_completion_valid_per_shell() {
    let cases = [
        ("bash", "complete"),
        ("zsh", "#compdef"),
        ("fish", "complete"),
        ("powershell", "Register-ArgumentCompleter"),
        ("elvish", "edit:completion:arg-completer"),
    ];
    for (shell, marker) in cases {
        let assert = shx().args(["completion", shell]).assert().success();
        let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
        let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
        assert!(
            stdout.contains(marker),
            "{shell}: missing {marker:?} in {} bytes",
            stdout.len()
        );
        assert!(stdout.contains("shx"), "{shell}: never mentions shx");
        assert!(
            stderr.is_empty(),
            "{shell}: stderr must stay silent: {stderr:?}"
        );
    }
}

/// Shell names are case-insensitive.
#[test]
fn t_604_completion_shell_case_insensitive() {
    for shell in ["Bash", "ZSH", "Fish"] {
        shx().args(["completion", shell]).assert().success();
    }
}

/// Unknown or missing shell: exit 2, stdout empty, stderr names the set.
#[test]
fn t_604_completion_usage_errors() {
    let assert = shx().args(["completion", "nushell"]).assert().code(2);
    assert!(assert.get_output().stdout.is_empty());
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(
        stderr.contains("bash") && stderr.contains("zsh"),
        "{stderr:?}"
    );

    let assert = shx().arg("completion").assert().code(2);
    assert!(assert.get_output().stdout.is_empty());
}

/// `shx man` prints a roff page naming the binary.
#[test]
fn t_604_man_is_roff() {
    let assert = shx().arg("man").assert().success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    let stderr = String::from_utf8_lossy(&assert.get_output().stderr);
    assert!(stdout.contains(".TH"), "roff title header");
    assert!(stdout.contains("shx"), "names the binary");
    assert!(stderr.is_empty(), "{stderr:?}");
}

/// Committed snapshots match runtime output byte-for-byte, so release
/// archives (T-607) can ship them without the binary.
#[test]
fn t_604_snapshots_match_runtime() {
    let root = workspace_root();
    let man_disk = fs::read_to_string(root.join("man/shx.1")).expect("man/shx.1");
    let assert = shx().arg("man").assert().success();
    let man_live = String::from_utf8_lossy(&assert.get_output().stdout);
    assert_eq!(man_live, man_disk, "man/shx.1 drifts from `shx man`");

    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let disk = fs::read_to_string(root.join(format!("contrib/completions/shx.{shell}")))
            .unwrap_or_else(|_| panic!("contrib/completions/shx.{shell}"));
        let assert = shx().args(["completion", shell]).assert().success();
        let live = String::from_utf8_lossy(&assert.get_output().stdout);
        assert_eq!(live, disk, "contrib/completions/shx.{shell} drifts");
    }
}

/// Snapshots exist for every supported shell plus the man page.
#[test]
fn t_604_archive_files_exist() {
    let root = workspace_root();
    assert!(root.join("man/shx.1").is_file(), "man/shx.1 missing");
    for shell in ["bash", "zsh", "fish", "powershell", "elvish"] {
        let path = root.join(format!("contrib/completions/shx.{shell}"));
        assert!(path.is_file(), "{} missing", path.display());
        assert!(
            fs::metadata(&path).expect("stat").len() > 100,
            "{} looks truncated",
            path.display()
        );
    }
}

/// Completions do not depend on config or HOME isolation.
#[test]
fn t_604_completion_needs_no_home() {
    let assert = shx()
        .env_clear()
        .current_dir(Path::new("/"))
        .args(["completion", "bash"])
        .assert()
        .success();
    let stdout = String::from_utf8_lossy(&assert.get_output().stdout);
    assert!(stdout.contains("complete"));
}
