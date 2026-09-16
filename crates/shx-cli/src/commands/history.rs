//! `shx history` — list / show / export / prune / purge.

use std::fs;

use shx_config::load;
use shx_core::{Interaction, PrunePolicy, RiskLevel, Scope};
use shx_memory::MemoryStore;

use crate::pipeline::{flag_overrides, open_store};
use crate::render::{EXIT_ERROR, EXIT_OK, EXIT_USAGE};
use crate::{Cli, Commands, HistoryCmd};

/// Dispatch `shx history …`.
pub fn run(cli: &Cli) -> i32 {
    let Some(Commands::History {
        limit,
        project,
        global,
        risk,
        grep,
        json,
        jsonl,
        cmd,
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

    match cmd {
        None | Some(HistoryCmd::List) => list(
            &store,
            *limit,
            *project,
            *global,
            risk.as_deref(),
            grep.as_deref(),
            *json,
            *jsonl,
            cli,
        ),
        Some(HistoryCmd::Show {
            id,
            json: show_json,
        }) => show(&store, *id, *json || *show_json),
        Some(HistoryCmd::Export { json, jsonl, out }) => {
            let as_jsonl = *jsonl || !*json;
            export(&store, *limit, as_jsonl, out.as_deref())
        }
        Some(HistoryCmd::Prune {
            older_than,
            keep_danger,
        }) => prune(
            &store,
            older_than.as_deref(),
            *keep_danger,
            loaded.config.memory.retention_days,
        ),
        Some(HistoryCmd::Purge { all }) => purge(&store, *all, cli.yes),
    }
}

fn scope(project: bool, _global: bool, cli: &Cli) -> Scope {
    if project {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let (_, pid) = shx_memory::scope::resolve_project_scope(cli.project.as_deref(), &cwd);
        Scope::Project { id: pid }
    } else {
        Scope::Tool
    }
}

fn fetch(
    store: &impl MemoryStore,
    limit: usize,
    project: bool,
    global: bool,
    grep: Option<&str>,
    cli: &Cli,
) -> Result<Vec<Interaction>, String> {
    let sc = scope(project, global, cli);
    if let Some(q) = grep {
        store.search(q, limit, sc).map_err(|e| e.to_string())
    } else {
        store.recent(limit, sc).map_err(|e| e.to_string())
    }
}

fn filter_risk(rows: Vec<Interaction>, risk: Option<&str>) -> Result<Vec<Interaction>, String> {
    let Some(r) = risk else {
        return Ok(rows);
    };
    let want = match r {
        "safe" => RiskLevel::Safe,
        "review" => RiskLevel::Review,
        "danger" => RiskLevel::Danger,
        other => {
            return Err(format!(
                "invalid --risk {other}; accepted: safe, review, danger"
            ));
        }
    };
    Ok(rows.into_iter().filter(|i| i.risk_level == want).collect())
}

#[allow(clippy::too_many_arguments)] // mirrors `shx history` clap flags
fn list(
    store: &impl MemoryStore,
    limit: usize,
    project: bool,
    global: bool,
    risk: Option<&str>,
    grep: Option<&str>,
    json: bool,
    jsonl: bool,
    cli: &Cli,
) -> i32 {
    let rows =
        match fetch(store, limit, project, global, grep, cli).and_then(|r| filter_risk(r, risk)) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_USAGE;
            }
        };
    emit(&rows, json, jsonl, None)
}

fn show(store: &shx_memory::SqliteStore, id: i64, json: bool) -> i32 {
    match store.get(id) {
        Ok(Some(row)) => {
            if json {
                match serde_json::to_string(&row) {
                    Ok(s) => println!("{s}"),
                    Err(e) => {
                        eprintln!("shx: {e}");
                        return EXIT_ERROR;
                    }
                }
            } else {
                print_human(&row);
            }
            EXIT_OK
        }
        Ok(None) => {
            eprintln!("shx: no interaction {id}");
            EXIT_ERROR
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}

fn export(store: &impl MemoryStore, limit: usize, jsonl: bool, out: Option<&str>) -> i32 {
    let rows = match store.recent(limit.max(10_000), Scope::Tool) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_ERROR;
        }
    };
    emit(&rows, !jsonl, jsonl, out)
}

fn emit(rows: &[Interaction], json: bool, jsonl: bool, out: Option<&str>) -> i32 {
    let body = if jsonl {
        let mut buf = String::new();
        for row in rows {
            match serde_json::to_string(row) {
                Ok(s) => {
                    buf.push_str(&s);
                    buf.push('\n');
                }
                Err(e) => {
                    eprintln!("shx: {e}");
                    return EXIT_ERROR;
                }
            }
        }
        buf
    } else if json {
        match serde_json::to_string(rows) {
            Ok(s) => format!("{s}\n"),
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        let mut buf = String::new();
        for row in rows {
            buf.push_str(&human_line(row));
            buf.push('\n');
        }
        buf
    };
    if let Some(path) = out {
        if let Err(e) = fs::write(path, &body) {
            eprintln!("shx: write {path}: {e}");
            return EXIT_ERROR;
        }
        eprintln!("wrote {path} ({} rows)", rows.len());
        EXIT_OK
    } else {
        print!("{body}");
        EXIT_OK
    }
}

fn human_line(row: &Interaction) -> String {
    let id = row.id.unwrap_or(0);
    format!(
        "{id}\t{}\t{}\t{} => {}",
        row.ts,
        format!("{:?}", row.risk_level).to_ascii_lowercase(),
        row.input_nl,
        row.output_cmd
    )
}

fn print_human(row: &Interaction) {
    println!("{}", human_line(row));
    if let Some(ex) = &row.explanation {
        eprintln!("{ex}");
    }
}

fn prune(
    store: &impl MemoryStore,
    older_than: Option<&str>,
    keep_danger: bool,
    default_days: u32,
) -> i32 {
    let days = match older_than {
        None => default_days,
        Some(s) => match parse_days(s) {
            Ok(d) => d,
            Err(e) => {
                eprintln!("shx: {e}");
                return EXIT_USAGE;
            }
        },
    };
    match store.prune(&PrunePolicy {
        retention_days: days,
        keep_danger,
    }) {
        Ok(r) => {
            eprintln!(
                "prune: deleted {} retained-danger-stripped {} vocab-decayed {}",
                r.interactions_deleted, r.danger_commands_stripped, r.vocab_decayed
            );
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}

fn parse_days(s: &str) -> Result<u32, String> {
    let t = s.trim().to_ascii_lowercase();
    let num = t
        .strip_suffix("days")
        .or_else(|| t.strip_suffix("day"))
        .or_else(|| t.strip_suffix('d'))
        .unwrap_or(&t)
        .trim();
    num.parse::<u32>()
        .map_err(|_| format!("invalid --older-than {s}; expected e.g. 180d"))
        .and_then(|n| {
            if n == 0 {
                Err("invalid --older-than 0".into())
            } else {
                Ok(n)
            }
        })
}

fn purge(store: &shx_memory::SqliteStore, all: bool, yes: bool) -> i32 {
    if !all {
        eprintln!("shx history purge: pass --all (and -y to confirm)");
        return EXIT_USAGE;
    }
    if !yes {
        eprintln!("shx history purge: re-run with -y --all to delete all stored interactions");
        return EXIT_USAGE;
    }
    match store.purge_all() {
        Ok(n) => {
            eprintln!("purged {n} interactions");
            EXIT_OK
        }
        Err(e) => {
            eprintln!("shx: {e}");
            EXIT_ERROR
        }
    }
}
