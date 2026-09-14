//! Redaction-at-write (docs/03-MEMORY.md §3, 04-SAFETY.md §3.1).
//!
//! Every insert path must run [`prepare_interaction`] / [`prepare_shell_cmd`]
//! so a raw secret cannot land on disk.

use shx_core::{Interaction, Redactor, SecretRedactor, ShellEntry, Snippet};

/// Copy `i` with `input_nl`, `output_cmd`, and `explanation` redacted.
pub fn prepare_interaction(i: &Interaction) -> Interaction {
    let r = SecretRedactor;
    let mut out = i.clone();
    out.input_nl = r.redact(&i.input_nl).into_owned();
    out.output_cmd = r.redact(&i.output_cmd).into_owned();
    out.explanation = i.explanation.as_ref().map(|e| r.redact(e).into_owned());
    out
}

/// Redact a shell-history command line.
pub fn prepare_shell_cmd(cmd: &str) -> String {
    SecretRedactor.redact(cmd).into_owned()
}

/// Redact a [`ShellEntry`] for insert.
pub fn prepare_shell_entry(e: &ShellEntry) -> ShellEntry {
    let mut out = e.clone();
    out.cmd = prepare_shell_cmd(&e.cmd);
    out
}

/// Copy `s` with `command` and `description` redacted. Name is trimmed.
pub fn prepare_snippet(s: &Snippet) -> Snippet {
    let r = SecretRedactor;
    let mut out = s.clone();
    out.name = s.name.trim().to_string();
    out.command = r.redact(&s.command).into_owned();
    out.description = s
        .description
        .as_ref()
        .map(|d| r.redact(d.trim()).into_owned())
        .filter(|d| !d.is_empty());
    out
}

/// True if `s` still contains a typical key-shaped literal (test helper).
pub fn looks_unredacted_secret(s: &str) -> bool {
    s.contains("sk-TEST") || s.contains("AKIA") || s.contains("ghp_") || s.contains("xoxb-")
}
