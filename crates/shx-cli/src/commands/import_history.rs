//! `shx import-history` — opt-in shell-history ingest (T-605, Scope::Shell).
//!
//! Parses zsh / bash / fish history files into `(ts, cmd)` entries and
//! inserts them via `SqliteStore::record_shell`, which redacts every line
//! **before** insert. The summary goes to stderr; stdout stays empty (there
//! is no command to print). `--dry-run` parses and counts without opening
//! the database at all, so it cannot write.
//!
//! Format notes (all deterministic, all edge-tested below):
//! - zsh: `: <epoch>:<elapsed>;<cmd>` extended lines and plain lines.
//!   Backslash-newline continuations are unfolded (a trailing `\` joins the
//!   next line with `\n`; an even trailing `\\` is a literal backslash).
//!   History files never store the cwd, so `cwd` is always `None`.
//! - bash: plain lines; a line that is exactly `#<digits>` is a timestamp
//!   for the *next* command (HISTTIMEFORMAT). Anything else starting with
//!   `#` is an ordinary (comment) command. Multi-line commands are stored
//!   as separate lines by bash and import as separate entries.
//! - fish: `- cmd: <cmd>` stanzas with an optional following `when: <epoch>`
//!   (missing `when` means ts 0). Surrounding double quotes are stripped and
//!   `\\` / `\"` unescaped; anything else is a documented limitation.
//! - Blank lines are ignored everywhere (neither imported nor skipped).

use std::fs;
use std::path::{Path, PathBuf};

use shx_config::load;
use shx_core::{Redactor, SecretRedactor};

use crate::pipeline::{flag_overrides, open_store};
use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands};

/// One parsed history entry: unix seconds (0 when the file stores none).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Entry {
    ts: i64,
    cmd: String,
}

/// Supported `--shell` values (spec §3: zsh | bash | fish).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HistoryShell {
    Zsh,
    Bash,
    Fish,
}

impl HistoryShell {
    /// Value stored in the `source` column.
    fn name(self) -> &'static str {
        match self {
            Self::Zsh => "zsh",
            Self::Bash => "bash",
            Self::Fish => "fish",
        }
    }

    /// Default history file under `home`.
    fn default_file(self, home: &Path) -> PathBuf {
        match self {
            Self::Zsh => home.join(".zsh_history"),
            Self::Bash => home.join(".bash_history"),
            Self::Fish => home.join(".local/share/fish/fish_history"),
        }
    }

    /// Parse file text into entries plus a malformed-line count.
    fn parse(self, text: &str) -> (Vec<Entry>, usize) {
        match self {
            Self::Zsh => parse_zsh(text),
            Self::Bash => parse_bash(text),
            Self::Fish => parse_fish(text),
        }
    }
}

/// Resolve `--shell`, inferring from `$SHELL` when omitted.
fn resolve_shell(raw: Option<&str>) -> Result<HistoryShell, String> {
    if let Some(s) = raw {
        return parse_shell_name(s);
    }
    let shell = std::env::var("SHELL").unwrap_or_default();
    let base = shell.rsplit('/').next().unwrap_or("").to_ascii_lowercase();
    match base.as_str() {
        "zsh" => Ok(HistoryShell::Zsh),
        "bash" => Ok(HistoryShell::Bash),
        "fish" => Ok(HistoryShell::Fish),
        _ => Err(format!(
            "cannot infer shell from SHELL={shell:?}; pass --shell zsh|bash|fish"
        )),
    }
}

fn parse_shell_name(raw: &str) -> Result<HistoryShell, String> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "zsh" => Ok(HistoryShell::Zsh),
        "bash" => Ok(HistoryShell::Bash),
        "fish" => Ok(HistoryShell::Fish),
        _ => Err(format!(
            "invalid --shell {raw:?}; accepted: zsh, bash, fish"
        )),
    }
}

/// Split into logical lines, unfolding `\`-newline continuations. A line
/// ending in an odd run of backslashes continues (one backslash dropped,
/// joined with `\n`); an even run is literal.
fn unfold_continuations(text: &str) -> Vec<String> {
    let mut logical = Vec::new();
    let mut current = String::new();
    for line in text.split('\n') {
        let stripped = line.strip_suffix('\r').unwrap_or(line);
        let trailing = stripped.len() - stripped.trim_end_matches('\\').len();
        if trailing % 2 == 1 {
            current.push_str(&stripped[..stripped.len() - 1]);
            current.push('\n');
        } else {
            current.push_str(stripped);
            logical.push(std::mem::take(&mut current));
        }
    }
    if !current.trim().is_empty() {
        logical.push(current);
    }
    logical
}

/// Parse one zsh logical line: `None` = blank (ignored), `Some(Ok)` = entry,
/// `Some(Err)` = malformed.
fn parse_zsh_line(line: &str) -> Option<Result<Entry, ()>> {
    if line.trim().is_empty() {
        return None;
    }
    let Some(rest) = line.strip_prefix(':') else {
        return Some(Ok(Entry {
            ts: 0,
            cmd: line.to_string(),
        }));
    };
    let Some((meta, cmd)) = rest.split_once(';') else {
        return Some(Err(()));
    };
    let Some((epoch, _elapsed)) = meta.trim().split_once(':') else {
        return Some(Err(()));
    };
    let Ok(ts) = epoch.trim().parse::<i64>() else {
        return Some(Err(()));
    };
    if cmd.is_empty() {
        return Some(Err(()));
    }
    Some(Ok(Entry {
        ts,
        cmd: cmd.to_string(),
    }))
}

fn parse_zsh(text: &str) -> (Vec<Entry>, usize) {
    let mut entries = Vec::new();
    let mut skipped = 0usize;
    for line in unfold_continuations(text) {
        match parse_zsh_line(&line) {
            None => {}
            Some(Ok(e)) => entries.push(e),
            Some(Err(())) => skipped += 1,
        }
    }
    (entries, skipped)
}

fn parse_bash(text: &str) -> (Vec<Entry>, usize) {
    let mut entries = Vec::new();
    let mut pending_ts: Option<i64> = None;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line.trim().is_empty() {
            continue;
        }
        if let Some(digits) = line.strip_prefix('#')
            && !digits.is_empty()
            && digits.bytes().all(|b| b.is_ascii_digit())
        {
            // `#<digits>` is a HISTTIMEFORMAT timestamp for the next command.
            // Epochs that fail to parse (overflow) fall through as commands
            // so no user data is ever dropped as "malformed".
            if let Ok(ts) = digits.parse::<i64>() {
                pending_ts = Some(ts);
                continue;
            }
        }
        entries.push(Entry {
            ts: pending_ts.take().unwrap_or(0),
            cmd: line.to_string(),
        });
    }
    (entries, 0)
}

fn parse_fish(text: &str) -> (Vec<Entry>, usize) {
    let mut entries: Vec<Entry> = Vec::new();
    let mut skipped = 0usize;
    for raw in text.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix("- cmd:") {
            let cmd = unquote_fish(rest.trim());
            if cmd.is_empty() {
                skipped += 1;
                continue;
            }
            entries.push(Entry { ts: 0, cmd });
            continue;
        }
        if let Some(rest) = line.strip_prefix("when:") {
            let value = rest.trim();
            if !value.is_empty()
                && value.bytes().all(|b| b.is_ascii_digit())
                && let Ok(ts) = value.parse::<i64>()
            {
                // `when:` follows its `- cmd:` stanza: attach to the most
                // recent entry that has no timestamp yet.
                if let Some(last) = entries.last_mut()
                    && last.ts == 0
                {
                    last.ts = ts;
                }
                continue;
            }
            skipped += 1;
            continue;
        }
        skipped += 1;
    }
    (entries, skipped)
}

/// Strip one pair of surrounding double quotes and unescape `\\` / `\"`.
fn unquote_fish(cmd: &str) -> String {
    let inner = cmd
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .unwrap_or(cmd);
    if !inner.contains('\\') {
        return inner.to_string();
    }
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            match chars.next() {
                Some('\\') => out.push('\\'),
                Some('"') => out.push('"'),
                Some(other) => {
                    out.push('\\');
                    out.push(other);
                }
                None => out.push('\\'),
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// True when the store would change `cmd` (mirrors `record_shell` redaction).
fn is_redacted(cmd: &str) -> bool {
    SecretRedactor.redact(cmd).as_ref() != cmd
}

/// Dispatch `shx import-history`.
pub fn run(cli: &Cli) -> i32 {
    let Some(Commands::ImportHistory {
        shell,
        file,
        limit,
        dry_run,
    }) = &cli.command
    else {
        return EXIT_ERROR;
    };

    let shell = match resolve_shell(shell.as_deref()) {
        Ok(s) => s,
        Err(msg) => {
            eprintln!("shx import-history: {msg}");
            return EXIT_USAGE;
        }
    };
    let path = match file {
        Some(p) => PathBuf::from(p),
        None => match home_dir() {
            Some(h) => shell.default_file(&h),
            None => {
                eprintln!("shx import-history: cannot locate a home directory");
                return EXIT_ERROR;
            }
        },
    };
    let text = match fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("shx import-history: read {}: {e}", path.display());
            return EXIT_ERROR;
        }
    };
    let (mut entries, skipped) = shell.parse(&text);
    if let Some(n) = limit {
        // Most recent N: the file is chronological, so keep the tail.
        let keep = (*n).min(entries.len());
        entries = entries.split_off(entries.len() - keep);
    }
    let redacted = entries.iter().filter(|e| is_redacted(&e.cmd)).count();

    if *dry_run {
        eprintln!(
            "dry-run: would import {}, redacted {}, skipped {} malformed",
            entries.len(),
            redacted,
            skipped
        );
        return EXIT_OK;
    }

    let flags = flag_overrides(cli);
    let loaded = match load(flags) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_USAGE;
        }
    };
    let store = match open_store(&loaded.config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_ERROR;
        }
    };
    let source = shell.name();
    for entry in &entries {
        // `record_shell` redacts again before insert (defense in depth: the
        // count above used the same redactor, so the numbers agree).
        if let Err(e) = store.record_shell(entry.ts, None, &entry.cmd, None, source) {
            eprintln!("shx: {e}");
            return EXIT_ERROR;
        }
    }
    eprintln!(
        "imported {}, redacted {}, skipped {} malformed",
        entries.len(),
        redacted,
        skipped
    );
    EXIT_OK
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zsh_extended_plain_and_malformed() {
        let (entries, skipped) = parse_zsh(": 1700000000:0;git status\nls -la\n: nonsense\n");
        assert_eq!(
            entries,
            vec![
                Entry {
                    ts: 1700000000,
                    cmd: "git status".into()
                },
                Entry {
                    ts: 0,
                    cmd: "ls -la".into()
                },
            ]
        );
        assert_eq!(skipped, 1);
    }

    #[test]
    fn zsh_edge_lines() {
        // Empty command after `;` is malformed; blanks ignored, not counted.
        let (entries, skipped) = parse_zsh(": 1:2;\n\n   \n: 1:2;echo hi\n: 9:x\n");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].cmd, "echo hi");
        assert_eq!(skipped, 2, ": 1:2; and : 9:x");
        // Command text may itself contain `;` and `:`.
        let (entries, skipped) = parse_zsh(": 5:0;echo a:b; echo c\n");
        assert_eq!(skipped, 0);
        assert_eq!(entries[0].cmd, "echo a:b; echo c");
        assert_eq!(entries[0].ts, 5);
    }

    #[test]
    fn zsh_continuation_unfolding() {
        // Odd trailing backslash continues; even is literal.
        let (entries, skipped) = parse_zsh("echo one \\\nand two\nC:\\\\path\n");
        assert_eq!(skipped, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].cmd, "echo one \nand two");
        assert_eq!(entries[1].cmd, "C:\\\\path");
    }

    #[test]
    fn bash_timestamps_comments_and_trailing_ts() {
        let text = "#1700000000\ngit status\n# deploy to prod\nls\n#1700000001\n";
        let (entries, skipped) = parse_bash(text);
        assert_eq!(skipped, 0);
        assert_eq!(
            entries,
            vec![
                Entry {
                    ts: 1700000000,
                    cmd: "git status".into()
                },
                Entry {
                    ts: 0,
                    cmd: "# deploy to prod".into()
                },
                Entry {
                    ts: 0,
                    cmd: "ls".into()
                },
            ]
        );
    }

    #[test]
    fn bash_overflowing_timestamp_stays_a_command() {
        let (entries, skipped) = parse_bash("#99999999999999999999999\necho hi\n");
        assert_eq!(skipped, 0);
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].cmd, "#99999999999999999999999");
        assert_eq!(entries[1].ts, 0);
    }

    #[test]
    fn fish_stanzas_missing_when_and_bad_lines() {
        let text = "- cmd: git status\n  when: 1700000000\n- cmd: ls\n- cmd: \"echo hi\"\n  when: nope\njunk line\n";
        let (entries, skipped) = parse_fish(text);
        assert_eq!(
            entries,
            vec![
                Entry {
                    ts: 1700000000,
                    cmd: "git status".into()
                },
                Entry {
                    ts: 0,
                    cmd: "ls".into()
                },
                Entry {
                    ts: 0,
                    cmd: "echo hi".into()
                },
            ]
        );
        // Bad `when:` + unknown line; the orphan-ignored blank rules apply.
        assert_eq!(skipped, 2);
    }

    #[test]
    fn fish_when_before_cmd_is_ignored() {
        let (entries, skipped) = parse_fish("when: 1700000000\n- cmd: ls\n");
        assert_eq!(skipped, 0);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ts, 0);
    }

    #[test]
    fn fish_unquote_unescapes() {
        assert_eq!(unquote_fish("plain"), "plain");
        assert_eq!(unquote_fish("\"a b\""), "a b");
        assert_eq!(unquote_fish("\"say \\\"hi\\\"\""), "say \"hi\"");
        assert_eq!(unquote_fish("\"C:\\\\x\""), "C:\\x");
        assert_eq!(unquote_fish("\"dangling"), "\"dangling");
    }

    #[test]
    fn empty_inputs_parse_clean() {
        assert_eq!(parse_zsh(""), (Vec::new(), 0));
        assert_eq!(parse_bash(""), (Vec::new(), 0));
        assert_eq!(parse_fish(""), (Vec::new(), 0));
        assert_eq!(parse_zsh("\n\n"), (Vec::new(), 0));
    }

    #[test]
    fn shell_names_and_inference() {
        assert_eq!(parse_shell_name("zsh"), Ok(HistoryShell::Zsh));
        assert_eq!(parse_shell_name("BASH"), Ok(HistoryShell::Bash));
        assert!(parse_shell_name("powershell").is_err());
        assert_eq!(HistoryShell::Fish.name(), "fish");
    }

    #[test]
    fn redacted_detection_mirrors_store() {
        assert!(is_redacted("export TOKEN=sk-TESTFAKE0000000000000000"));
        assert!(!is_redacted("git status"));
    }
}
