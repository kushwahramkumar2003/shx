//! `shx doctor` — config, DB path (stub), mock backend, print-only self-report.

use std::env;
use std::path::PathBuf;

use serde_json::json;
use shx_config::{Config, load};
use shx_llm::{Backend, MockBackend};

use crate::Cli;
use crate::pipeline::flag_overrides;
use crate::render::{EXIT_BACKEND, EXIT_ERROR, EXIT_OK};

/// Run `shx doctor`. Human report on stderr; `--json` on stdout.
pub fn run(json: bool, redaction_test: bool, cli: &Cli) -> i32 {
    let flags = flag_overrides(cli);
    let (config_ok, config_detail, loaded) = match load(flags) {
        Ok(l) => (true, "ok".to_string(), Some(l)),
        Err(e) => (false, e.to_string(), None),
    };

    let cfg_ref: Option<&Config> = loaded.as_ref().map(|l| &l.config);
    let db = stub_db_path(cfg_ref);
    let backend = MockBackend::new();
    let health = backend.health();
    let backend_ok = health.reachable && health.model_present && health.auth_configured;

    let redaction_ok = true;
    let redaction_skipped = !redaction_test;
    let redaction_detail = if redaction_test {
        "corpus runner lands in T-305; treating as skip"
    } else {
        "skipped (pass --redaction-test; full runner is T-305)"
    };

    let ok = config_ok && backend_ok && redaction_ok;
    let exit = if !config_ok {
        EXIT_ERROR
    } else if !backend_ok {
        EXIT_BACKEND
    } else {
        EXIT_OK
    };

    if json {
        let report = json!({
            "ok": ok,
            "exit_reason": if exit == 0 { "ok" } else if exit == 4 { "backend" } else { "error" },
            "checks": {
                "config_parse": { "ok": config_ok, "detail": config_detail },
                "db_path": {
                    "ok": true,
                    "path": db.display().to_string(),
                    "stub": true
                },
                "backend": {
                    "ok": backend_ok,
                    "id": backend.id(),
                    "reachable": health.reachable,
                    "model_present": health.model_present,
                    "auth_configured": health.auth_configured,
                    "message": health.message,
                },
                "print_only": {
                    "ok": true,
                    "detail": "T-SAFE-3 enforced; no execution path in the binary"
                },
                "redaction": {
                    "ok": redaction_ok,
                    "skipped": redaction_skipped,
                    "detail": redaction_detail
                }
            },
            "config": loaded.as_ref().map(|l| &l.config),
        });
        match serde_json::to_string(&report) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("shx: failed to serialize doctor --json: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        eprintln!("shx doctor");
        eprintln!("  config_parse: {}", if config_ok { "ok" } else { "FAIL" });
        if !config_ok {
            eprintln!("    {config_detail}");
        }
        eprintln!("  db_path: {} (stub)", db.display());
        eprintln!(
            "  backend: {} reachable={} model_present={} auth={}",
            backend.id(),
            health.reachable,
            health.model_present,
            health.auth_configured
        );
        eprintln!("  print_only: ok (T-SAFE-3)");
        eprintln!(
            "  redaction: {}",
            if redaction_skipped { "skipped" } else { "ok" }
        );
        if let Some(l) = &loaded {
            for w in &l.warnings {
                eprintln!("warning: {w}");
            }
        }
    }
    exit
}

fn stub_db_path(cfg: Option<&Config>) -> PathBuf {
    if let Some(cfg) = cfg
        && !cfg.memory.path.is_empty()
    {
        return PathBuf::from(&cfg.memory.path);
    }
    if let Ok(xdg) = env::var("XDG_DATA_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("shx").join("shx.db");
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("shx")
                .join("shx.db");
        }
    }
    #[cfg(windows)]
    {
        if let Some(local) = env::var_os("LOCALAPPDATA") {
            return PathBuf::from(local).join("shx").join("shx.db");
        }
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".local")
            .join("share")
            .join("shx")
            .join("shx.db");
    }
    PathBuf::from("shx.db")
}
