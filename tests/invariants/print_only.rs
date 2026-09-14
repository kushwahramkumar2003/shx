//! T-SAFE-3 — print-only invariant (ADR-002).
//!
//! Greps the tree for process-spawning APIs. Failures are allow-listed only
//! with a justifying comment. Never delete this test.

use std::fs;
use std::path::{Path, PathBuf};

/// Relative paths (from workspace root) that may spawn a process.
///
/// - `xtask/`: the CI runner, never user-derived argv.
/// - `commands/doctor.rs`: T-007 ollama probe, fixed argv, no user text.
/// - `commands/config.rs`: T-007 `$EDITOR` on a path we own; the program is
///   not taken from the intent.
const ALLOW_LIST: &[&str] = &[
    "xtask/src/main.rs",
    "crates/shx-cli/src/commands/doctor.rs",
    "crates/shx-cli/src/commands/config.rs",
    "tests/invariants/print_only.rs",
];

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn is_allow_listed(rel: &str) -> bool {
    ALLOW_LIST
        .iter()
        .any(|p| rel == *p || rel.replace('\\', "/") == *p)
}

fn spawn_needles() -> Vec<String> {
    // Split so this file's own source is not a false positive if scanned.
    vec![
        format!("{}::{}", "process", "Command"),
        format!("{}::{}", "Command", "new"),
        format!("{}::{}", "libc", "system"),
        "execvp".to_string(),
        "popen(".to_string(),
        "sh -c".to_string(),
    ]
}

fn exec_flag_needles() -> Vec<String> {
    vec![
        "\"--run\"".to_string(),
        "\"--exec\"".to_string(),
        "long = \"run\"".to_string(),
        "long = \"exec\"".to_string(),
    ]
}

fn collect_rs(dir: &Path, out: &mut Vec<PathBuf>) {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let name = entry.file_name();
        if name == "target" || name == ".git" || name == ".commandcode" {
            continue;
        }
        if path.is_dir() {
            collect_rs(&path, out);
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

fn rel_path(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn line_hits(line: &str, needles: &[String]) -> bool {
    let trimmed = line.trim();
    if trimmed.starts_with("//") || trimmed.starts_with("///") {
        return false;
    }
    needles.iter().any(|n| line.contains(n))
}

/// A source line containing `Command::new` outside the allow-list must fail.
#[test]
fn string_fixture_detects_spawn() {
    let needles = spawn_needles();
    let evil = format!("let _ = std::{}(\"echo\");", "process::Command::new");
    assert!(line_hits(&evil, &needles), "fixture must be detected");
    assert!(
        !line_hits("// process::Command is forbidden here", &needles),
        "comments are not spawn sites"
    );
}

#[test]
fn t_safe_3_no_user_derived_spawn() {
    let root = workspace_root();
    let needles = spawn_needles();
    let mut files = Vec::new();
    collect_rs(&root, &mut files);
    let mut violations = Vec::new();
    for file in files {
        let rel = rel_path(&root, &file);
        if is_allow_listed(&rel) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line_hits(line, &needles) {
                violations.push(format!("{rel}:{}:{line}", i + 1));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "T-SAFE-3: spawn API outside allow-list:\n{}",
        violations.join("\n")
    );
}

#[test]
fn t_safe_3_no_run_or_exec_flag() {
    let root = workspace_root();
    let needles = exec_flag_needles();
    let cli = root.join("crates/shx-cli");
    let mut files = Vec::new();
    collect_rs(&cli, &mut files);
    let mut hits = Vec::new();
    for file in files {
        let rel = rel_path(&root, &file);
        if is_allow_listed(&rel) {
            continue;
        }
        let Ok(text) = fs::read_to_string(&file) else {
            continue;
        };
        for (i, line) in text.lines().enumerate() {
            if line_hits(line, &needles) {
                hits.push(format!("{rel}:{}:{line}", i + 1));
            }
        }
    }
    assert!(
        hits.is_empty(),
        "T-SAFE-3: --run/--exec flag must not exist:\n{}",
        hits.join("\n")
    );
}
