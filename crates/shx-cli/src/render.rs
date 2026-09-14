//! stdout = command (or `--json`); stderr = everything human-facing.

use std::io::{self, IsTerminal};

use serde::Serialize;
use shx_config::ColorMode;
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
/// Intent refused (not translated).
pub const EXIT_REFUSED: i32 = 6;

const ANSI_RED: &str = "\x1b[1;31m";
const ANSI_YELLOW: &str = "\x1b[1;33m";
const ANSI_RESET: &str = "\x1b[0m";

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
    /// `--exit-on-risk` or `safety.exit_on_risk`.
    pub exit_on_risk: bool,
    /// `safety.warn_on_risk` — print color banners on stderr.
    pub warn_on_risk: bool,
    /// Emit ANSI on stderr.
    pub color: bool,
}

/// Write the translation and return the process exit code.
pub fn render(out: &TranslateOut, opts: RenderOpts) -> i32 {
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }

    if let Some(reason) = &out.refused {
        if opts.json {
            match serde_json::to_string(&json_out(out, opts.exit_on_risk)) {
                Ok(s) => println!("{s}"),
                Err(e) => {
                    eprintln!("shx: failed to serialize --json: {e}");
                    return EXIT_ERROR;
                }
            }
        }
        eprintln!("refused: {reason}");
        return EXIT_REFUSED;
    }

    if opts.warn_on_risk
        && let Some(first) = out.candidates.first()
    {
        eprint!("{}", format_banner(&out.risk, &first.command, opts.color));
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
    }
    if opts.why {
        eprint!("{}", format_why(out));
    }

    if opts.exit_on_risk && out.risk.level >= RiskLevel::Review {
        EXIT_RISK
    } else {
        EXIT_OK
    }
}

/// Whether stderr should carry ANSI, honoring `NO_COLOR` and `CLICOLOR_FORCE`.
pub fn color_stderr(mode: ColorMode) -> bool {
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    match mode {
        ColorMode::Never => false,
        ColorMode::Always => true,
        ColorMode::Auto => {
            std::env::var_os("CLICOLOR_FORCE").is_some() || io::stderr().is_terminal()
        }
    }
}

/// Color-coded risk banner (`what` + `why`). Empty when level is Safe.
pub fn format_banner(risk: &shx_core::RiskAssessment, command: &str, color: bool) -> String {
    if risk.level < RiskLevel::Review {
        return String::new();
    }
    let label = match risk.level {
        RiskLevel::Danger => "DANGER",
        RiskLevel::Review => "REVIEW",
        RiskLevel::Safe => unreachable!(),
    };
    let painted = if color {
        let code = match risk.level {
            RiskLevel::Danger => ANSI_RED,
            _ => ANSI_YELLOW,
        };
        format!("{code}{label}{ANSI_RESET}")
    } else {
        label.to_string()
    };
    let mut s = format!("{painted}: {command}\n");
    for note in &risk.notes {
        s.push_str(&format!("  why: {note}\n"));
    }
    s
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
    let exit_reason = if out.refused.is_some() {
        "refused"
    } else if exit_on_risk && out.risk.level >= RiskLevel::Review {
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
            used: out.why.memory_used,
            entries: out.why.history.len() as u32,
            project_id: None,
            from_cache: out.from_cache,
        },
        latency_ms: out.latency_ms,
        exit_reason,
    }
}

/// `--why` block. Always stderr; never stdout.
pub fn format_why(out: &TranslateOut) -> String {
    let w = &out.why;
    let ports = w
        .ports
        .iter()
        .map(|p| p.to_string())
        .collect::<Vec<_>>()
        .join(", ");
    let prefer = if w.prefer_docker { "docker" } else { "host" };
    let mut s = String::new();
    s.push_str("why:\n");
    s.push_str(&format!(
        "  backend: {}  model: {}  from_cache: {}\n",
        out.backend_id, out.model, out.from_cache
    ));
    s.push_str("  routing: local-first (placeholder; T-403)\n");
    s.push_str(&format!(
        "  profile: name={} ports=[{ports}] prefer={prefer}\n",
        w.profile_name
    ));
    s.push_str(&format!(
        "  memory: used={} entries={} truncated={} tokens={}/{}\n",
        w.memory_used,
        w.history.len(),
        w.truncated,
        w.tokens,
        w.max_tokens
    ));
    if !w.history.is_empty() {
        s.push_str("  history:\n");
        for (id, input, cmd) in &w.history {
            let id = id.unwrap_or(0);
            s.push_str(&format!("    - #{id} {input} => {cmd}\n"));
        }
    }
    if !w.vocabulary.is_empty() {
        s.push_str("  vocabulary:\n");
        for (term, exp) in &w.vocabulary {
            s.push_str(&format!("    - {term} = {exp}\n"));
        }
    }
    if !w.snippets.is_empty() {
        s.push_str("  snippets:\n");
        for name in &w.snippets {
            s.push_str(&format!("    - {name}\n"));
        }
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::{Candidate, RiskAssessment, RiskLevel};

    use crate::pipeline::{TranslateOut, WhyInfo};

    #[test]
    fn why_snapshot() {
        let out = TranslateOut {
            input: "run pg on 7000".into(),
            candidates: vec![Candidate {
                command: "docker run pg".into(),
                explanation: "start postgres".into(),
                confidence: 0.9,
            }],
            risk: RiskAssessment {
                level: RiskLevel::Safe,
                rules: vec![],
                notes: vec![],
            },
            backend_id: "mock".into(),
            model: "fixture".into(),
            latency_ms: 0,
            from_cache: false,
            warnings: vec![],
            raw: String::new(),
            why: WhyInfo {
                memory_used: true,
                truncated: false,
                tokens: 12,
                max_tokens: 1500,
                history: vec![(Some(1), "run pg on 7000".into(), "docker run pg".into())],
                vocabulary: vec![("pg".into(), "postgres".into())],
                snippets: vec!["pg-up".into()],
                profile_name: "default".into(),
                ports: vec![3000, 5432, 7000],
                prefer_docker: true,
            },
            refused: None,
        };
        assert_eq!(format_why(&out), include_str!("../tests/golden/why.txt"));
    }

    #[test]
    fn danger_banner_snapshot() {
        let risk = RiskAssessment {
            level: RiskLevel::Danger,
            rules: vec![shx_core::RuleId::new("del.recursive-force")],
            notes: vec!["recursive force-delete targeting /, ~, $HOME, or .".into()],
        };
        assert_eq!(
            format_banner(&risk, "rm -rf /", false),
            include_str!("../tests/golden/danger-banner.txt")
        );
        let painted = format_banner(&risk, "rm -rf /", true);
        assert!(
            painted.contains("\u{1b}[1;31mDANGER\u{1b}[0m"),
            "danger banner is red: {painted:?}"
        );
        assert!(
            !painted.contains("rm -rf /\u{1b}"),
            "command itself is not colored: {painted:?}"
        );
    }

    #[test]
    fn review_banner_is_yellow() {
        let risk = RiskAssessment {
            level: RiskLevel::Review,
            rules: vec![shx_core::RuleId::new("proc.kill-minus-one")],
            notes: vec!["kill -9 -1 / kill -1".into()],
        };
        let plain = format_banner(&risk, "kill -9 -1", false);
        assert!(plain.starts_with("REVIEW: kill -9 -1\n"));
        assert!(plain.contains("  why: kill -9 -1 / kill -1\n"));
        let painted = format_banner(&risk, "kill -9 -1", true);
        assert!(painted.contains("\u{1b}[1;33mREVIEW\u{1b}[0m"));
    }

    #[test]
    fn safe_has_no_banner() {
        let risk = RiskAssessment {
            level: RiskLevel::Safe,
            rules: vec![],
            notes: vec![],
        };
        assert_eq!(format_banner(&risk, "echo hi", false), "");
    }

    #[test]
    fn color_never_and_no_color_env() {
        assert!(!color_stderr(ColorMode::Never));
    }
}
