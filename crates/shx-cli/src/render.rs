//! stdout = command (or `--json`); stderr = everything human-facing.

use serde::Serialize;
use shx_core::{RiskLevel, RuleId};

use crate::pipeline::TranslateOut;

/// CLI exit codes (docs/05-CLI-SPEC.md §5).
pub const EXIT_OK: i32 = 0;
/// Unexpected internal failure.
pub const EXIT_ERROR: i32 = 1;
/// Usage / bad flags / `--cloud` unconfigured / invalid config.
pub const EXIT_USAGE: i32 = 2;
/// `--exit-on-risk` and risk ≥ Review.
pub const EXIT_RISK: i32 = 3;
/// Backend unavailable.
pub const EXIT_BACKEND: i32 = 4;
/// Memory degraded (translation still printed). Slot for T-201.
#[allow(dead_code)]
pub const EXIT_MEMORY: i32 = 5;
/// Intent refused. Slot for T-304.
#[allow(dead_code)]
pub const EXIT_REFUSED: i32 = 6;

/// How to print a [`TranslateOut`].
#[derive(Debug, Clone, Copy)]
pub struct RenderOpts {
    /// `--json`.
    pub json: bool,
    /// `-q`.
    pub quiet: bool,
    /// `-v`.
    pub verbose: bool,
    /// `--why`.
    pub why: bool,
    /// `--exit-on-risk`.
    pub exit_on_risk: bool,
}

/// Write the translation and return the process exit code.
pub fn render(out: &TranslateOut, opts: RenderOpts) -> i32 {
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }

    if opts.json {
        match serde_json::to_string(&json_out(out, opts.exit_on_risk)) {
            Ok(s) => println!("{s}"),
            Err(e) => {
                eprintln!("shx: failed to serialize --json: {e}");
                return EXIT_ERROR;
            }
        }
    } else {
        for c in &out.candidates {
            println!("{}", c.command);
        }
        if !opts.quiet
            && let Some(first) = out.candidates.first()
            && !first.explanation.is_empty()
        {
            eprintln!("{}", first.explanation);
        }
        if opts.verbose && !out.raw.is_empty() {
            eprintln!("raw: {}", out.raw);
        }
        if opts.why {
            eprintln!(
                "why: backend={} model={} memory=off from_cache={}",
                out.backend_id, out.model, out.from_cache
            );
        }
    }

    if opts.exit_on_risk && out.risk.level >= RiskLevel::Review {
        EXIT_RISK
    } else {
        EXIT_OK
    }
}

#[derive(Serialize)]
struct JsonOut<'a> {
    version: u32,
    input: &'a str,
    commands: Vec<JsonCommand<'a>>,
    risk: JsonRisk<'a>,
    backend: JsonBackend<'a>,
    memory: JsonMemory,
    latency_ms: u64,
    exit_reason: &'static str,
}

#[derive(Serialize)]
struct JsonCommand<'a> {
    command: &'a str,
    explanation: &'a str,
    confidence: f32,
}

#[derive(Serialize)]
struct JsonRisk<'a> {
    level: RiskLevel,
    rules: Vec<&'a RuleId>,
    notes: &'a [String],
}

#[derive(Serialize)]
struct JsonBackend<'a> {
    id: &'a str,
    model: &'a str,
    escalated_from: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonMemory {
    used: bool,
    entries: u32,
    project_id: Option<String>,
    from_cache: bool,
}

fn json_out<'a>(out: &'a TranslateOut, exit_on_risk: bool) -> JsonOut<'a> {
    let exit_reason = if exit_on_risk && out.risk.level >= RiskLevel::Review {
        "risk"
    } else {
        "ok"
    };
    JsonOut {
        version: 1,
        input: &out.input,
        commands: out
            .candidates
            .iter()
            .map(|c| JsonCommand {
                command: &c.command,
                explanation: &c.explanation,
                confidence: c.confidence,
            })
            .collect(),
        risk: JsonRisk {
            level: out.risk.level,
            rules: out.risk.rules.iter().collect(),
            notes: &out.risk.notes,
        },
        backend: JsonBackend {
            id: &out.backend_id,
            model: &out.model,
            escalated_from: None,
        },
        memory: JsonMemory {
            used: false,
            entries: 0,
            project_id: None,
            from_cache: out.from_cache,
        },
        latency_ms: out.latency_ms,
        exit_reason,
    }
}
