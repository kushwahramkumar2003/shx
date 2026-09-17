//! `shx -i` refine sessions (T-602).
//!
//! One invocation, many turns: the initial intent comes from argv and each
//! follow-up line comes from stdin. Every turn runs the same translate
//! pipeline and records it under one shared `session_id`, so the session
//! groups its interactions in memory and later turns see earlier ones as
//! context. The final command is recorded exactly once, by its own turn —
//! session end performs no extra write.
//!
//! A blank line or EOF ends the session (exit code of the last turn, or 2
//! when no turn ran). Every input line is translated verbatim: there are no
//! magic `quit` words. SIGINT keeps its default disposition — the process
//! terminates immediately with no cleanup needed (turns committed so far
//! stay grouped; SQLite writes are atomic), which is the clean Ctrl-C exit.
//! Nothing is ever executed (ADR-002); stdout carries one command per turn.

use std::io::{self, BufRead, IsTerminal, Write};
use std::sync::atomic::{AtomicU64, Ordering};

use shx_config::load;

use crate::Cli;
use crate::pipeline::{PipelineError, flag_overrides};
use crate::render::{EXIT_ERROR, EXIT_USAGE, RenderOpts, color_stderr};

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Follow-up prompt on stderr (TTY only, so piped sessions stay quiet).
const REFINE_PROMPT: &str = "refine> ";

/// Fresh per-process session id (`sess-<pid>-<nanos>-<n>`), std-only.
pub(crate) fn new_session_id() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let n = SESSION_COUNTER.fetch_add(1, Ordering::SeqCst);
    format!("sess-{}-{nanos}-{n}", std::process::id())
}

/// Join argv intent words; `None` when no initial intent was given.
fn split_initial(intent: &[String]) -> Option<String> {
    let text = intent.join(" ");
    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Dispatch `shx -i`.
pub fn run(cli: &Cli) -> i32 {
    let flags = flag_overrides(cli);
    let loaded = match load(flags) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_USAGE;
        }
    };
    for w in &loaded.warnings {
        // Always stderr, including with --json (warnings are not the JSON object).
        eprintln!("warning: {w}");
    }
    let opts = RenderOpts {
        json: cli.json,
        quiet: cli.quiet,
        verbose: cli.verbose,
        why: cli.why,
        exit_on_risk: loaded.config.safety.exit_on_risk,
        warn_on_risk: loaded.config.safety.warn_on_risk,
        color: color_stderr(loaded.config.ui.color),
    };
    let session_id = new_session_id();
    let mut code = EXIT_USAGE;
    let mut turns = 0u32;

    if let Some(first) = split_initial(&cli.intent) {
        code = turn(cli, &loaded.config, &opts, &session_id, &first);
        turns += 1;
        if code == EXIT_USAGE || code == EXIT_ERROR {
            return code;
        }
    }

    let stdin = io::stdin();
    let mut lines = stdin.lock().lines();
    loop {
        if stdin_is_tty() {
            eprint!("{REFINE_PROMPT}");
            let _ = io::stderr().flush();
        }
        let line = match lines.next() {
            None => break,
            Some(Err(e)) => {
                eprintln!("shx: failed to read stdin: {e}");
                return EXIT_ERROR;
            }
            Some(Ok(l)) => l,
        };
        if line.trim().is_empty() {
            break;
        }
        code = turn(cli, &loaded.config, &opts, &session_id, &line);
        turns += 1;
        if code == EXIT_USAGE || code == EXIT_ERROR {
            return code;
        }
    }

    if turns == 0 {
        eprintln!("shx -i: usage: shx -i \"<intent>\" then follow-up lines on stdin");
        return EXIT_USAGE;
    }
    code
}

fn stdin_is_tty() -> bool {
    io::stdin().is_terminal()
}

/// Run one session turn: same pipeline as single-shot, tagged with the
/// shared session id, rendered with the session-wide options.
fn turn(
    cli: &Cli,
    config: &shx_config::Config,
    opts: &RenderOpts,
    session_id: &str,
    text: &str,
) -> i32 {
    let mut turn_cli = cli.clone();
    turn_cli.intent = vec![text.to_string()];
    turn_cli.interactive = false;
    match crate::pipeline::run(&turn_cli, config, Vec::new(), Some(session_id)) {
        Ok(out) => crate::render::render(&out, *opts),
        Err(PipelineError::Usage(msg)) => {
            eprintln!("shx: {msg}");
            EXIT_USAGE
        }
        Err(PipelineError::Backend(msg)) => {
            eprintln!("shx: {msg}");
            crate::render::EXIT_BACKEND
        }
        Err(PipelineError::Other(msg)) => {
            eprintln!("shx: {msg}");
            EXIT_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_ids_are_prefixed_and_unique() {
        let a = new_session_id();
        let b = new_session_id();
        assert!(a.starts_with("sess-"), "{a}");
        assert!(b.starts_with("sess-"), "{b}");
        assert_ne!(a, b, "counter keeps same-process sessions distinct");
        assert_ne!(a.trim(), "", "never blank");
    }

    #[test]
    fn split_initial_joins_or_absent() {
        assert_eq!(split_initial(&[]), None);
        assert_eq!(split_initial(&["  ".into()]), None);
        assert_eq!(
            split_initial(&["run".into(), "pg".into()]),
            Some("run pg".into())
        );
        // Kept verbatim (no trimming): the pipeline owns normalization.
        assert_eq!(split_initial(&["  ls  ".into()]), Some("  ls  ".into()));
    }
}
