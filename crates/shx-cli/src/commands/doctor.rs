//! `shx doctor` — config, DB path (stub), local backend probe, print-only.

use std::env;
use std::path::PathBuf;

use serde::Serialize;
use serde_json::json;
use shx_config::{Config, load};
use shx_core::{Redactor, SecretRedactor};
use shx_llm::{Backend, Health, MockBackend, OllamaBackend, OllamaSettings};

use crate::Cli;
use crate::pipeline::flag_overrides;
use crate::render::{EXIT_BACKEND, EXIT_ERROR, EXIT_OK};

/// Bundled T-SAFE-4 corpus (`tests/corpus/redaction.txt`).
const REDACTION_CORPUS: &str = include_str!("../../../../tests/corpus/redaction.txt");

/// Run `shx doctor`. Human report on stderr; `--json` on stdout.
pub fn run(json: bool, redaction_test: bool, cli: &Cli) -> i32 {
    let flags = flag_overrides(cli);
    let (config_ok, config_detail, loaded) = match load(flags) {
        Ok(l) => (true, "ok".to_string(), Some(l)),
        Err(e) => (false, e.to_string(), None),
    };

    let cfg_ref: Option<&Config> = loaded.as_ref().map(|l| &l.config);
    let db = stub_db_path(cfg_ref);
    let (backend_id, health) = probe_backend(cli.offline, cfg_ref);
    let backend_ok = health.reachable && health.model_present && health.auth_configured;

    let redaction_skipped = !redaction_test;
    let redaction = if redaction_test {
        Some(run_redaction_corpus(REDACTION_CORPUS))
    } else {
        None
    };
    let redaction_ok = redaction.as_ref().is_none_or(|r| r.ok());
    let redaction_detail = match &redaction {
        None => "skipped (pass --redaction-test)".to_string(),
        Some(r) if r.ok() => format!("{}/{} passed", r.passed(), r.total()),
        Some(r) => format!("FAIL {}/{}", r.passed(), r.total()),
    };

    let ok = config_ok && backend_ok && redaction_ok;
    let exit = if !config_ok {
        EXIT_ERROR
    } else if !backend_ok {
        EXIT_BACKEND
    } else if !redaction_ok {
        EXIT_ERROR
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
                    "id": backend_id,
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
                    "passed": redaction.as_ref().map(RedactionReport::passed),
                    "total": redaction.as_ref().map(RedactionReport::total),
                    "patterns": redaction.as_ref().map(|r| &r.patterns),
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
            "  backend: {backend_id} reachable={} model_present={} auth={}",
            health.reachable, health.model_present, health.auth_configured
        );
        if let Some(msg) = &health.message {
            eprintln!("    {msg}");
        }
        eprintln!("  print_only: ok (T-SAFE-3)");
        eprintln!(
            "  redaction: {}",
            if redaction_skipped {
                "skipped"
            } else {
                redaction_detail.as_str()
            }
        );
        if let Some(report) = &redaction {
            for p in &report.patterns {
                let status = if p.ok() { "pass" } else { "FAIL" };
                eprintln!("    {}: {status} ({}/{})", p.id, p.passed, p.total);
                for f in &p.failures {
                    eprintln!("      in: {}", f.input);
                    eprintln!("      want: {}", f.want);
                    eprintln!("      got: {}", f.got);
                }
            }
        }
        if let Some(l) = &loaded {
            for w in &l.warnings {
                eprintln!("warning: {w}");
            }
        }
    }
    exit
}

/// `--offline` uses the mock; otherwise probe Ollama from config (T-103).
fn probe_backend(offline: bool, cfg: Option<&Config>) -> (&'static str, Health) {
    if offline {
        return ("mock", MockBackend::new().health());
    }
    let settings = match cfg {
        Some(c) => OllamaSettings {
            base_url: c.backend.local.base_url.clone(),
            model: c.backend.local.model.clone(),
            keep_alive: c.backend.local.keep_alive.clone(),
            num_ctx: c.backend.local.num_ctx,
            timeout_ms: c.backend.local.timeout_ms.min(2_000),
        },
        None => OllamaSettings {
            timeout_ms: 2_000,
            ..OllamaSettings::default()
        },
    };
    let backend = OllamaBackend::new(settings);
    (backend.id(), backend.health())
}

/// One corpus line that did not match the expected redaction.
#[derive(Debug, Clone, Serialize)]
struct RedactionFailure {
    input: String,
    want: String,
    got: String,
}

/// Pass/fail for one `#`-headed pattern group in the corpus.
#[derive(Debug, Clone, Serialize)]
struct PatternResult {
    id: String,
    ok: bool,
    passed: u32,
    total: u32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    failures: Vec<RedactionFailure>,
}

impl PatternResult {
    fn ok(&self) -> bool {
        self.ok
    }
}

/// Full `--redaction-test` report.
#[derive(Debug, Clone, Serialize)]
struct RedactionReport {
    patterns: Vec<PatternResult>,
}

impl RedactionReport {
    fn passed(&self) -> u32 {
        self.patterns.iter().map(|p| p.passed).sum()
    }

    fn total(&self) -> u32 {
        self.patterns.iter().map(|p| p.total).sum()
    }

    fn ok(&self) -> bool {
        !self.patterns.is_empty() && self.patterns.iter().all(PatternResult::ok)
    }
}

fn run_redaction_corpus(text: &str) -> RedactionReport {
    let r = SecretRedactor;
    let mut patterns: Vec<PatternResult> = Vec::new();
    let mut current = "ungrouped".to_string();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        if let Some(rest) = line.strip_prefix('#') {
            let name = rest.trim();
            if name.is_empty() || is_file_header(name) {
                continue;
            }
            current = pattern_id(name);
            continue;
        }
        let Some((left, right)) = line.split_once('→') else {
            continue;
        };
        let input = left.trim();
        let want = right.trim();
        let got = r.redact(input);
        let slot = pattern_slot(&mut patterns, &current);
        slot.total += 1;
        if got.as_ref() == want {
            slot.passed += 1;
        } else {
            slot.failures.push(RedactionFailure {
                input: input.to_string(),
                want: want.to_string(),
                got: got.into_owned(),
            });
        }
        slot.ok = slot.passed == slot.total;
    }

    patterns.retain(|p| p.total > 0);
    RedactionReport { patterns }
}

fn pattern_slot<'a>(patterns: &'a mut Vec<PatternResult>, id: &str) -> &'a mut PatternResult {
    if let Some(i) = patterns.iter().position(|p| p.id == id) {
        return &mut patterns[i];
    }
    patterns.push(PatternResult {
        id: id.to_string(),
        ok: true,
        passed: 0,
        total: 0,
        failures: Vec::new(),
    });
    let i = patterns.len() - 1;
    &mut patterns[i]
}

fn is_file_header(name: &str) -> bool {
    name.starts_with("T-SAFE") || name.starts_with("Positives")
}

fn pattern_id(name: &str) -> String {
    if name.starts_with("---") {
        return "negatives".into();
    }
    name.split(['(', '—'])
        .next()
        .unwrap_or(name)
        .trim()
        .to_string()
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bundled_corpus_all_pass() {
        let report = run_redaction_corpus(REDACTION_CORPUS);
        assert!(report.ok(), "bundled corpus must pass: {report:?}");
        assert!(report.total() >= 16);
        let ids: Vec<&str> = report.patterns.iter().map(|p| p.id.as_str()).collect();
        assert!(ids.contains(&"aws-key"));
        assert!(ids.contains(&"github"));
        assert!(ids.contains(&"negatives"));
        assert!(
            report.patterns.iter().all(|p| p.ok()),
            "every pattern pass: {ids:?}"
        );
    }

    #[test]
    fn failing_pattern_is_not_ok() {
        let corpus = "# aws-key\nrun with AKIAIOSFODNN7EXAMPLE00 now → not-masked\n";
        let report = run_redaction_corpus(corpus);
        assert!(!report.ok());
        assert_eq!(report.passed(), 0);
        assert_eq!(report.total(), 1);
        assert_eq!(report.patterns[0].id, "aws-key");
        assert!(!report.patterns[0].ok);
        assert_eq!(report.patterns[0].failures.len(), 1);
    }
}
