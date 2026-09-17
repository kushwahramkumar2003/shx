//! `shx snippet save|list|show|rm`.
//!
//! Snippets are context for the model, never auto-executed as `shx <name>`.

use shx_config::load;
use shx_core::Snippet;
use shx_memory::MemoryStore;

use crate::pipeline::{flag_overrides, open_store};
use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands, SnippetCmd};

/// Dispatch `shx snippet …`.
pub fn run(cli: &Cli) -> i32 {
    let Some(Commands::Snippet { cmd }) = &cli.command else {
        return EXIT_ERROR;
    };
    let Some(cmd) = cmd else {
        eprintln!(
            "shx snippet: usage: shx snippet save|list|show|rm\n\
             snippets are context, not shortcuts: `shx <name>` is never auto-resolved"
        );
        return EXIT_USAGE;
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

    match cmd {
        SnippetCmd::Save {
            name,
            command,
            description,
        } => save(&store, name, command, description.as_deref()),
        SnippetCmd::List { json } => list(&store, cli.json || *json),
        SnippetCmd::Show { name, copy, json } => {
            show(&store, name, *copy || cli.copy, cli.json || *json)
        }
        SnippetCmd::Rm { name } => rm(&store, name),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn save(
    store: &shx_memory::SqliteStore,
    name: &str,
    command: &str,
    description: Option<&str>,
) -> i32 {
    let name = name.trim();
    let command = command.trim();
    if name.is_empty() {
        eprintln!("shx snippet save: name is empty");
        return EXIT_USAGE;
    }
    if command.is_empty() {
        eprintln!("shx snippet save: --command is empty");
        return EXIT_USAGE;
    }
    let snippet = Snippet {
        id: None,
        name: name.to_string(),
        command: command.to_string(),
        description: description
            .map(str::trim)
            .filter(|d| !d.is_empty())
            .map(str::to_string),
        created_ts: now_ms(),
        use_count: 0,
    };
    match store.upsert_snippet(&snippet) {
        Ok(_) => {
            eprintln!("saved {name}");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}

fn list(store: &impl MemoryStore, json: bool) -> i32 {
    let mut rows = match store.snippets() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_ERROR;
        }
    };
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    if json {
        match serde_json::to_string(&rows) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        for s in &rows {
            println!("{}\t{}", s.name, s.command);
        }
    }
    EXIT_OK
}

fn show(store: &shx_memory::SqliteStore, name: &str, copy: bool, json: bool) -> i32 {
    match store.get_snippet(name) {
        Ok(Some(s)) => emit_show(&s, copy, json),
        Ok(None) => {
            eprintln!("shx: no snippet {name}");
            EXIT_ERROR
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}

fn emit_show(s: &Snippet, copy: bool, json: bool) -> i32 {
    if json && !copy {
        match serde_json::to_string(s) {
            Ok(body) => println!("{body}"),
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_ERROR;
            }
        }
        return EXIT_OK;
    }
    let cmd = s.command.trim_end_matches('\n');
    if copy {
        // Command channel only (`shx snippet show <name> --copy | pbcopy`);
        // the clipboard attempt is best-effort on top. Never exec.
        println!("{cmd}");
        if let Err(e) = crate::render::copy_to_clipboard(cmd) {
            eprintln!("warning: --copy ignored: {e}");
        }
        return EXIT_OK;
    }
    println!("{}\t{cmd}", s.name);
    if let Some(d) = &s.description {
        eprintln!("{d}");
    }
    EXIT_OK
}

fn rm(store: &shx_memory::SqliteStore, name: &str) -> i32 {
    match store.delete_snippet(name) {
        Ok(0) => {
            eprintln!("shx snippet rm: no snippet {name}");
            EXIT_ERROR
        }
        Ok(_) => {
            eprintln!("removed {name}");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}
