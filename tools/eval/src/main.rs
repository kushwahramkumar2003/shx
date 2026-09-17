//! `shx-eval` binary: run the T6 harness from the command line.
//!
//! ```sh
//! cargo run -p shx-eval -- --fixtures tools/eval/fixtures/translate.json
//! cargo run -p shx-eval -- --live --fixtures tools/eval/fixtures/translate.json
//! ```
//!
//! Exit codes: `0` the quality gate passed, `1` an eval failure (quality
//! gate, case errors, or live backend unavailable), `2` usage or fixture
//! errors. The results table (or `--json` object) goes to stdout;
//! diagnostics go to stderr.

#![forbid(unsafe_code)]

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use clap::Parser;
use shx_eval::run::{Report, load_cases, run_eval};
use shx_llm::{Backend, MockBackend, OllamaBackend, OllamaSettings};

/// Score translate.json fixtures through a chosen backend (docs/08-TESTING.md §7).
#[derive(Debug, Parser)]
#[command(name = "shx-eval", version, about)]
struct Cli {
    /// Fixture file to score.
    #[arg(long, default_value = "tools/eval/fixtures/translate.json")]
    fixtures: PathBuf,
    /// Translate with live Ollama (`SHX_LIVE_MODEL`/`SHX_LIVE_URL`) instead of the mock.
    #[arg(long)]
    live: bool,
    /// Repeat passes (pass 1 scores quality; later passes score cache).
    #[arg(long, default_value_t = 3)]
    passes: usize,
    /// Fail (exit 1) when the exact-match rate drops below this 0.0–1.0 floor.
    #[arg(long, default_value_t = 0.0)]
    min_exact: f64,
    /// Print the aggregate metrics object as JSON instead of the table.
    #[arg(long)]
    json: bool,
}

const EXIT_OK: i32 = 0;
const EXIT_EVAL_FAILED: i32 = 1;
const EXIT_USAGE: i32 = 2;
const DEFAULT_LIVE_TIMEOUT_MS: u64 = 60_000;

fn live_settings() -> OllamaSettings {
    let mut settings = OllamaSettings {
        timeout_ms: env::var("SHX_LIVE_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_LIVE_TIMEOUT_MS),
        ..OllamaSettings::default()
    };
    if let Ok(model) = env::var("SHX_LIVE_MODEL")
        && !model.is_empty()
    {
        settings.model = model;
    }
    if let Ok(url) = env::var("SHX_LIVE_URL")
        && !url.is_empty()
    {
        settings.base_url = url;
    }
    settings
}

fn trunc(s: &str, max: usize) -> String {
    let mut out: String = s.chars().take(max).collect();
    if s.chars().count() > max {
        out = out.chars().take(max.saturating_sub(3)).collect();
        out.push_str("...");
    }
    out
}

fn render_table(report: &Report) -> String {
    let m = &report.metrics;
    let mut out = String::new();
    out.push_str(&format!(
        "{:<22} {:<10} {:>8}  {:<6}  command\n",
        "id", "tier", "ms", "cached"
    ));
    out.push_str(&format!("{}\n", "-".repeat(100)));
    for row in &report.rows {
        out.push_str(&format!(
            "{:<22} {:<10} {:>8.3}  {:<6}  {}\n",
            trunc(&row.id, 22),
            row.tier.label(),
            row.ms,
            if row.cached { "yes" } else { "no" },
            trunc(&row.command, 44),
        ));
    }
    out.push_str(&format!("{}\n", "-".repeat(100)));
    out.push_str(&format!(
        "cases: {}  passes: {}  backend: {}  model: {}\n",
        m.n_cases, m.passes, m.backend, m.model
    ));
    out.push_str(&format!(
        "exact {}/{} ({:.1}%) | regex {}/{} ({:.1}%) | acceptable {}/{} ({:.1}%)\n",
        m.exact,
        m.n_cases,
        m.exact_rate * 100.0,
        m.regex_matched,
        m.n_cases,
        m.regex_rate * 100.0,
        m.acceptable,
        m.n_cases,
        m.acceptable_rate * 100.0,
    ));
    out.push_str(&format!(
        "risk_misclassified: {}  errors: {}\n",
        m.risk_misclassified, m.errors
    ));
    if !m.risk_ids.is_empty() {
        out.push_str(&format!("risk_ids: {}\n", m.risk_ids.join(", ")));
    }
    if !m.error_ids.is_empty() {
        out.push_str(&format!("error_ids: {}\n", m.error_ids.join(", ")));
    }
    out.push_str(&format!(
        "latency_p50_ms: {:.3}  latency_p95_ms: {:.3}  tokens_total: {}\n",
        m.latency_p50_ms, m.latency_p95_ms, m.tokens_total
    ));
    out.push_str(&format!(
        "cache: {}/{} repeat lookups hit ({:.1}%)\n",
        m.cache_hits,
        m.cache_lookups,
        m.cache_hit_rate * 100.0
    ));
    out
}

/// Classified startup failure: usage/config (exit 2) vs eval (exit 1).
#[derive(Debug)]
enum Fail {
    /// Bad flags or unreadable/invalid fixtures.
    Usage(String),
    /// Live backend unavailable.
    Eval(String),
}

fn run(cli: &Cli) -> Result<Report, Fail> {
    if cli.passes == 0 {
        return Err(Fail::Usage("--passes must be >= 1".into()));
    }
    if !(0.0..=1.0).contains(&cli.min_exact) {
        return Err(Fail::Usage("--min-exact must be within 0.0–1.0".into()));
    }
    let cases = load_cases(&cli.fixtures).map_err(|e| Fail::Usage(e.to_string()))?;
    if cli.live {
        let settings = live_settings();
        let backend = OllamaBackend::new(settings.clone());
        let health = backend.health();
        if !health.reachable {
            return Err(Fail::Eval(format!(
                "live backend unreachable at {} (start ollama or set SHX_LIVE_URL)",
                settings.base_url
            )));
        }
        if !health.model_present {
            return Err(Fail::Eval(
                health
                    .message
                    .unwrap_or_else(|| format!("run: ollama pull {}", settings.model)),
            ));
        }
        Ok(run_eval(&cases, &backend, &settings.model, cli.passes))
    } else {
        let backend = MockBackend::new();
        Ok(run_eval(&cases, &backend, "fixture", cli.passes))
    }
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(&cli) {
        Err(Fail::Usage(msg)) => {
            eprintln!("shx-eval: {msg}");
            ExitCode::from(EXIT_USAGE as u8)
        }
        Err(Fail::Eval(msg)) => {
            eprintln!("shx-eval: {msg}");
            ExitCode::from(EXIT_EVAL_FAILED as u8)
        }
        Ok(report) => {
            if cli.json {
                match serde_json::to_string_pretty(&report.metrics) {
                    Ok(doc) => println!("{doc}"),
                    Err(e) => {
                        eprintln!("shx-eval: serialize --json: {e}");
                        return ExitCode::from(EXIT_EVAL_FAILED as u8);
                    }
                }
            } else {
                print!("{}", render_table(&report));
            }
            if !report.metrics.error_ids.is_empty() {
                eprintln!(
                    "shx-eval: {} case error(s): {}",
                    report.metrics.errors,
                    report.metrics.error_ids.join(", ")
                );
                return ExitCode::from(EXIT_EVAL_FAILED as u8);
            }
            if report.metrics.exact_rate < cli.min_exact {
                eprintln!(
                    "shx-eval: exact rate {:.3} below --min-exact {:.3}",
                    report.metrics.exact_rate, cli.min_exact
                );
                return ExitCode::from(EXIT_EVAL_FAILED as u8);
            }
            ExitCode::from(EXIT_OK as u8)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::render_table;
    use shx_eval::run::{Report, Row, Tier};
    use shx_eval::scoring::Metrics;

    fn sample_report() -> Report {
        Report {
            metrics: Metrics {
                version: 1,
                backend: "mock".into(),
                model: "fixture".into(),
                n_cases: 2,
                passes: 3,
                exact: 1,
                regex_matched: 1,
                acceptable: 2,
                exact_rate: 0.5,
                regex_rate: 0.5,
                acceptable_rate: 1.0,
                risk_misclassified: 0,
                risk_ids: Vec::new(),
                errors: 0,
                error_ids: Vec::new(),
                latency_p50_ms: 0.01,
                latency_p95_ms: 0.03,
                tokens_total: 0,
                cache_lookups: 4,
                cache_hits: 2,
                cache_hit_rate: 0.5,
            },
            rows: vec![
                Row {
                    id: "git-status".into(),
                    tier: Tier::Exact,
                    ms: 0.012,
                    cached: true,
                    command: "git status".into(),
                },
                Row {
                    id: "ls-hidden".into(),
                    tier: Tier::Regex,
                    ms: 0.009,
                    cached: true,
                    command: "ls -la".into(),
                },
            ],
        }
    }

    #[test]
    fn table_contains_all_metric_rows() {
        let table = render_table(&sample_report());
        for needle in [
            "git-status",
            "exact",
            "regex",
            "cases: 2",
            "backend: mock",
            "exact 1/2 (50.0%)",
            "risk_misclassified: 0",
            "latency_p50_ms:",
            "cache: 2/4 repeat lookups hit (50.0%)",
        ] {
            assert!(table.contains(needle), "missing {needle:?}:\n{table}");
        }
    }

    #[test]
    fn json_shape_has_version() {
        let doc = serde_json::to_value(&sample_report().metrics).expect("metrics serializes");
        assert_eq!(doc["version"], 1);
        assert_eq!(doc["backend"], "mock");
        assert_eq!(doc["n_cases"], 2);
        assert!(doc["exact_rate"].is_number());
        assert!(doc["cache_hit_rate"].is_number());
    }
}
