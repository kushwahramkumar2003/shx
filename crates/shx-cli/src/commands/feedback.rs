//! `shx feedback <id> good|bad [--note ...] [--executed] [--accepted]`.
//!
//! Records acceptance, execution, and implicit vocabulary learning
//! (docs/03-MEMORY.md §5). stdout stays empty; all human output goes to
//! stderr (ADR-001). This module never executes a command (ADR-002).

use shx_config::load;
use shx_core::Verdict;
use shx_memory::MemoryStore;

use crate::pipeline::{flag_overrides, open_store};
use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands};

const USAGE: &str =
    "shx feedback: usage: shx feedback <id> good|bad [--note \"<text>\"] [--executed] [--accepted]";

/// Dispatch `shx feedback`.
pub fn run(cli: &Cli) -> i32 {
    let Some(Commands::Feedback {
        id,
        verdict,
        note,
        executed,
        accepted,
    }) = &cli.command
    else {
        return EXIT_ERROR;
    };

    let Some(interaction_id) = id else {
        eprintln!("{USAGE}");
        return EXIT_USAGE;
    };

    let parsed_verdict = match verdict.as_deref() {
        None => None,
        Some(raw) => match parse_verdict(raw) {
            Some(v) => Some(v),
            None => {
                eprintln!("{USAGE}");
                eprintln!("shx feedback: invalid verdict {raw:?}; accepted: good, bad");
                return EXIT_USAGE;
            }
        },
    };

    if parsed_verdict.is_none() && !executed && !accepted {
        eprintln!("{USAGE}");
        return EXIT_USAGE;
    }
    if parsed_verdict.is_none() && note.is_some() {
        eprintln!("{USAGE}");
        eprintln!("shx feedback: --note requires a good|bad verdict");
        return EXIT_USAGE;
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

    let mut vocab_touched = 0u64;
    if let Some(v) = parsed_verdict {
        match store.feedback(*interaction_id, v, note.as_deref()) {
            Ok(()) => {
                // `feedback` already applied vocabulary learning; re-derive the
                // count for the summary without a second write by reading the
                // interaction terms. The count is best-effort: failures here
                // must not fail the feedback itself.
                vocab_touched = vocab_touched_for(&store, *interaction_id);
            }
            Err(e) => {
                return feedback_store_error(&e.to_string(), *interaction_id);
            }
        }
    }

    if *executed && let Err(e) = store.set_executed(*interaction_id, true) {
        return feedback_store_error(&e.to_string(), *interaction_id);
    }
    if *accepted && let Err(e) = store.set_accepted(*interaction_id, true) {
        return feedback_store_error(&e.to_string(), *interaction_id);
    }

    let verdict_word = match parsed_verdict {
        Some(Verdict::Good) => "good",
        Some(Verdict::Bad) => "bad",
        None => "flag-only",
    };
    eprintln!(
        "feedback {interaction_id} {verdict_word} (accepted={}, executed={}, vocab-updated={vocab_touched})",
        accepted_state(parsed_verdict, *accepted),
        executed_state(*executed),
    );
    EXIT_OK
}

fn parse_verdict(raw: &str) -> Option<Verdict> {
    match raw.trim().to_ascii_lowercase().as_str() {
        "good" => Some(Verdict::Good),
        "bad" => Some(Verdict::Bad),
        _ => None,
    }
}

fn accepted_state(verdict: Option<Verdict>, accepted_flag: bool) -> &'static str {
    if accepted_flag {
        "1"
    } else {
        match verdict {
            Some(Verdict::Good) => "1",
            Some(Verdict::Bad) => "0",
            None => "unchanged",
        }
    }
}

fn executed_state(executed_flag: bool) -> &'static str {
    if executed_flag { "1" } else { "unchanged" }
}

fn feedback_store_error(msg: &str, id: i64) -> i32 {
    if msg.contains(&format!("no interaction {id}")) {
        eprintln!("shx: no interaction {id}");
        EXIT_ERROR
    } else {
        eprintln!("shx: {msg}");
        EXIT_ERROR
    }
}

/// Best-effort count of vocabulary rows matching the interaction's terms.
///
/// Used only for the stderr summary; returns 0 when the interaction cannot
/// be read (the feedback itself already succeeded). Called after the write,
/// so a `good` on a brand-new term already sees its inserted row.
fn vocab_touched_for(store: &shx_memory::SqliteStore, id: i64) -> u64 {
    let Ok(Some(row)) = store.get(id) else {
        return 0;
    };
    let terms = shx_memory::vocab::extract_terms(&row.input_nl);
    if terms.is_empty() {
        return 0;
    }
    let mut n = 0u64;
    for term in &terms {
        let owned = term.clone();
        if let Ok(rows) = store.vocabulary(&[owned]) {
            n += rows.len() as u64;
        }
    }
    n
}

#[cfg(test)]
mod tests {
    use super::parse_verdict;
    use shx_core::Verdict;

    #[test]
    fn verdict_parsing_is_case_insensitive_and_trimmed() {
        assert_eq!(parse_verdict("good"), Some(Verdict::Good));
        assert_eq!(parse_verdict("GOOD"), Some(Verdict::Good));
        assert_eq!(parse_verdict("  bad  "), Some(Verdict::Bad));
        assert_eq!(parse_verdict("Bad"), Some(Verdict::Bad));
        assert_eq!(parse_verdict("great"), None);
        assert_eq!(parse_verdict(""), None);
        assert_eq!(parse_verdict("good bad"), None);
    }
}
