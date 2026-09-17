//! T-606: shell wrapper invariants — ≤30 lines, exit 3/4 respected, never
//! `eval`, install documented.
//!
//! Static assertions only (portable: no shell needed). Behavior was verified
//! manually against bash and zsh with a stub `shx`: buffer fills on exit 0,
//! stays untouched with a message on 3/4/other, empty buffer is a no-op.

use std::fs;
use std::path::PathBuf;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn read_wrapper(name: &str) -> String {
    let path = workspace_root().join(format!("contrib/shell/{name}"));
    fs::read_to_string(&path).unwrap_or_else(|_| panic!("{} missing", path.display()))
}

/// Both wrappers fit the ≤30-line budget with margin.
#[test]
fn t_606_wrappers_fit_line_budget() {
    for name in ["shx.zsh", "shx.bash"] {
        let text = read_wrapper(name);
        let lines = text.lines().count();
        assert!(
            lines <= 30,
            "{name}: {lines} lines exceeds the 30-line budget"
        );
        assert!(!text.trim().is_empty(), "{name}: empty");
    }
}

/// Neither wrapper may execute anything: no `eval`, no command substitution
/// of the *result*, no pipe-to-shell. (`$(shx …)` captures stdout only.)
#[test]
fn t_606_wrappers_never_eval() {
    for name in ["shx.zsh", "shx.bash"] {
        let text = read_wrapper(name);
        for (i, line) in text.lines().enumerate() {
            let code = line.split('#').next().unwrap_or("");
            // NB: two needles are split so this file itself stays T-SAFE-3
            // clean (same pattern as tests/invariants/print_only.rs).
            for needle in [
                "eval",
                "`",
                concat!("sh ", "-c"),
                "| sh",
                "| bash",
                "| zsh",
                concat!("exec", "vp"),
                concat!("pop", "en("),
            ] {
                assert!(
                    !code.contains(needle),
                    "{name}:{}: forbidden {needle:?}: {line:?}",
                    i + 1
                );
            }
        }
    }
}

/// Exit 3 (risk) and 4 (backend down) leave the edit buffer alone with a
/// message; only exit 0 writes it — exactly one write site per file.
#[test]
fn t_606_wrappers_respect_exit_3_and_4() {
    let zsh = read_wrapper("shx.zsh");
    assert_eq!(
        zsh.matches("BUFFER=$out").count(),
        1,
        "single buffer write site"
    );
    assert!(zsh.contains("code == 3") || zsh.contains("code==3"));
    assert!(zsh.contains("code == 4") || zsh.contains("code==4"));
    assert!(zsh.contains("zle -N"), "registered as a widget");
    assert!(zsh.contains("bindkey"), "bound to a key");
    assert!(
        zsh.contains("--exit-on-risk"),
        "risk gate must surface exit 3"
    );

    let bash = read_wrapper("shx.bash");
    assert_eq!(
        bash.matches("READLINE_LINE=$out").count(),
        1,
        "single buffer write site"
    );
    assert!(bash.contains("-eq 3"));
    assert!(bash.contains("-eq 4"));
    assert!(bash.contains("bind -x"), "readline binding");
    assert!(bash.contains("READLINE_POINT"), "cursor follows the fill");
    assert!(
        bash.contains("--exit-on-risk"),
        "risk gate must surface exit 3"
    );
}

/// Each wrapper carries its exact install line.
#[test]
fn t_606_wrappers_document_install() {
    for name in ["shx.zsh", "shx.bash"] {
        let text = read_wrapper(name);
        assert!(
            text.contains(&format!("source /path/to/contrib/shell/{name}")),
            "{name}: missing install line"
        );
    }
}

/// The CLI spec documents the same install lines as the files.
#[test]
fn t_606_spec_documents_install() {
    let spec =
        fs::read_to_string(workspace_root().join("docs/05-CLI-SPEC.md")).expect("05-CLI-SPEC.md");
    for name in ["contrib/shell/shx.zsh", "contrib/shell/shx.bash"] {
        assert!(spec.contains(name), "spec never mentions {name}");
    }
    assert!(spec.contains("Ctrl-G"), "spec names the binding");
}
