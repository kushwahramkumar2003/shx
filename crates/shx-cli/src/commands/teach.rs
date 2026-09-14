//! `shx teach` / `--forget` / `--list`.

use shx_config::load;
use shx_core::VocabSource;
use shx_memory::{MemoryStore, vocab};

use crate::pipeline::{flag_overrides, open_store};
use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands};

/// Dispatch `shx teach`.
pub fn run(cli: &Cli) -> i32 {
    let Some(Commands::Teach {
        forget,
        list,
        json,
        args,
    }) = &cli.command
    else {
        return EXIT_ERROR;
    };
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

    if *list {
        return list_vocab(&store, *json);
    }
    if let Some(term) = forget {
        return forget_term(&store, term);
    }
    if args.len() != 2 {
        eprintln!("shx teach: usage: shx teach <term> <expansion>");
        return EXIT_USAGE;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let entry = vocab::taught(&args[0], &args[1], now);
    match store.upsert_vocabulary(&entry) {
        Ok(()) => {
            eprintln!("taught {} → {}", entry.term, entry.expansion);
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}

fn list_vocab(store: &impl MemoryStore, json: bool) -> i32 {
    let rows = match store.vocabulary(&[]) {
        Ok(mut v) => {
            v.sort_by(vocab::cmp_rank);
            v
        }
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_ERROR;
        }
    };
    if json {
        match serde_json::to_string(&rows) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        for e in &rows {
            let src = match e.source {
                VocabSource::Taught => "taught",
                VocabSource::Learned => "learned",
                VocabSource::Imported => "imported",
            };
            println!("{}\t{}\t{:.2}\t{src}", e.term, e.expansion, e.weight);
        }
    }
    EXIT_OK
}

fn forget_term(store: &shx_memory::SqliteStore, term: &str) -> i32 {
    match store.forget_vocabulary(term) {
        Ok(0) => {
            eprintln!("shx teach --forget: no rows for {term}");
            EXIT_ERROR
        }
        Ok(n) => {
            eprintln!("forgot {term} ({n} rows)");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}
