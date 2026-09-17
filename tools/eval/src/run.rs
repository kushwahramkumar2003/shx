//! Fixture loading and the backend-agnostic scoring loop.
//!
//! Each fixture intent is served up to `passes` times through one shared
//! [`InMemoryStore`]: pass 1 scores quality (exact / regex / acceptable),
//! risk, errors, and cold latency; later passes score the fast-path cache.
//! Serving mirrors `shx-cli/src/pipeline.rs` (prompt → backend → risk →
//! record) without any network, subprocess, or wall-clock dependence except
//! for the latency statistic itself.

use std::collections::BTreeSet;
use std::fs;
use std::path::Path;
use std::time::Instant;

use shx_core::{
    ContextBundle, EnvInfo, Intent, IntentFlags, Interaction, Profile, PromptBuilder,
    RiskClassifier, RiskLevel, SecretRedactor, TranslateRequest,
};
use shx_llm::Backend;
use shx_memory::{CacheQuery, InMemoryStore, MemoryStore};

use crate::EvalError;
use crate::scoring::{
    METRICS_VERSION, Metrics, RawFixture, acceptable_match, exact_match, is_risk_misclassified,
    percentile, rate, regex_match,
};

/// One validated fixture case.
#[derive(Debug, Clone)]
pub struct Case {
    id: String,
    intent: String,
    os: String,
    shell: String,
    cwd: String,
    profile_name: String,
    ports: Vec<u16>,
    prefer_docker: bool,
    notes: String,
    expected: crate::scoring::Expected,
}

/// Scoring tier of one served command (best tier hit wins for the table).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    /// Trimmed command equals an alternative exactly.
    Exact,
    /// Command matches a `*` glob (or substring) alternative.
    Regex,
    /// Same leading program as the reference, flags/paths differ.
    Acceptable,
    /// Served but matched nothing.
    Miss,
    /// Backend error or empty command.
    Error,
}

impl Tier {
    /// Short table label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Exact => "exact",
            Self::Regex => "regex",
            Self::Acceptable => "acceptable",
            Self::Miss => "miss",
            Self::Error => "error",
        }
    }
}

/// One table row (pass-1 serve plus steady-state cache state).
#[derive(Debug, Clone)]
pub struct Row {
    /// Fixture id.
    pub id: String,
    /// Best tier hit on pass 1.
    pub tier: Tier,
    /// Pass-1 serve latency in milliseconds.
    pub ms: f64,
    /// Cache hit on the final repeat pass (`false` with a single pass).
    pub cached: bool,
    /// Pass-1 command (or the backend error string when `tier` is `Error`).
    pub command: String,
}

/// Full harness output: aggregate [`Metrics`] plus one [`Row`] per case.
#[derive(Debug, Clone)]
pub struct Report {
    /// Aggregate numbers (`--json` prints this object).
    pub metrics: Metrics,
    /// Per-case rows in fixture order.
    pub rows: Vec<Row>,
}

/// Read, parse, and validate a fixture file.
pub fn load_cases(path: &Path) -> Result<Vec<Case>, EvalError> {
    let raw = fs::read_to_string(path).map_err(|source| EvalError::Read {
        path: path.to_path_buf(),
        source,
    })?;
    let fixtures: Vec<RawFixture> =
        serde_json::from_str(&raw).map_err(|source| EvalError::Parse {
            path: path.to_path_buf(),
            source,
        })?;
    if fixtures.is_empty() {
        return Err(EvalError::Schema(format!(
            "{}: no cases (want at least 1)",
            path.display()
        )));
    }
    let mut cases = Vec::with_capacity(fixtures.len());
    let mut seen = BTreeSet::new();
    for (i, fix) in fixtures.into_iter().enumerate() {
        let id = fix
            .id
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| format!("case-{}", i + 1));
        if !seen.insert(id.clone()) {
            return Err(EvalError::Schema(format!("duplicate case id {id:?}")));
        }
        if fix.intent.trim().is_empty() {
            return Err(EvalError::Schema(format!(
                "case {i} ({id}): intent must be non-empty"
            )));
        }
        if fix.expected_command.is_empty() {
            return Err(EvalError::Schema(format!(
                "case {i} ({id}): expected_command must not be empty"
            )));
        }
        cases.push(Case {
            id,
            intent: fix.intent,
            os: fix.env.os,
            shell: fix.env.shell,
            cwd: fix.env.cwd,
            profile_name: fix.profile.name,
            ports: fix.profile.ports,
            prefer_docker: fix.profile.prefer_docker,
            notes: fix.profile.notes,
            expected: fix.expected_command,
        });
    }
    Ok(cases)
}

fn request_for(case: &Case) -> TranslateRequest {
    let intent = Intent {
        text: case.intent.clone(),
        force_backend: None,
        count: 1,
        flags: IntentFlags::default(),
    };
    let ctx = ContextBundle {
        env: EnvInfo {
            os: case.os.clone(),
            shell: case.shell.clone(),
            cwd: case.cwd.clone(),
            git_root: None,
            in_container: false,
        },
        profile: Profile {
            name: case.profile_name.clone(),
            ports: case.ports.clone(),
            prefer_docker: case.prefer_docker,
            notes: case.notes.clone(),
        },
        history: vec![],
        vocabulary: vec![],
        snippets: vec![],
        shell: vec![],
    };
    PromptBuilder.build(&intent, &ctx, &SecretRedactor)
}

fn query_for<'a>(case: &'a Case) -> CacheQuery<'a> {
    CacheQuery {
        intent: &case.intent,
        os: &case.os,
        shell: &case.shell,
        cwd: &case.cwd,
        project_id: None,
        // The harness records whatever backend it runs (mock offline,
        // ollama live); both are servable, mirroring `--offline`.
        allow_mock: true,
        force_local: false,
        force_cloud: false,
    }
}

fn tier_of(case: &Case, command: &str) -> Tier {
    if exact_match(&case.expected, command) {
        Tier::Exact
    } else if regex_match(&case.expected, command) {
        Tier::Regex
    } else if acceptable_match(&case.expected, command) {
        Tier::Acceptable
    } else {
        Tier::Miss
    }
}

/// Serve every case `passes` times and score the run.
///
/// Quality, risk, and errors come from pass 1 only; cache hits/lookups from
/// passes 2..=`passes`; latency covers every serve and tokens every
/// translate call. `model` labels the translating model in [`Metrics`].
pub fn run_eval(cases: &[Case], backend: &dyn Backend, model: &str, passes: usize) -> Report {
    let passes = passes.max(1);
    let store = InMemoryStore::new();
    let mut ts: i64 = 1_700_000_000_000;

    let mut rows: Vec<Row> = Vec::with_capacity(cases.len());
    let mut exact = 0usize;
    let mut regex_matched = 0usize;
    let mut acceptable = 0usize;
    let mut risk_ids = Vec::new();
    let mut error_ids = Vec::new();
    let mut latencies: Vec<f64> = Vec::with_capacity(cases.len() * passes);
    let mut tokens_total: u64 = 0;
    let mut cache_lookups = 0usize;
    let mut cache_hits = 0usize;

    for pass in 1..=passes {
        for case in cases {
            let query = query_for(case);
            let lookup_started = Instant::now();
            let hit = store.cache_lookup(&query).unwrap_or(None);
            if pass > 1 {
                cache_lookups += 1;
            }
            if let Some(hit) = hit {
                if pass > 1 {
                    cache_hits += 1;
                }
                latencies.push(lookup_started.elapsed().as_secs_f64() * 1000.0);
                if pass == passes
                    && let Some(row) = rows.iter_mut().find(|r| r.id == case.id)
                {
                    row.cached = true;
                }
                ts += 1;
                let rec = Interaction {
                    id: None,
                    ts,
                    session_id: "eval".into(),
                    project_id: None,
                    cwd: case.cwd.clone(),
                    os: case.os.clone(),
                    shell: case.shell.clone(),
                    input_nl: case.intent.clone(),
                    output_cmd: hit.command.clone(),
                    explanation: hit.explanation.clone(),
                    backend: hit.backend.clone(),
                    model: hit.model.clone(),
                    confidence: hit.confidence,
                    latency_ms: 0,
                    risk_level: RiskLevel::Safe,
                    risk_notes: Vec::new(),
                    from_cache: true,
                    accepted: None,
                    executed: None,
                    tags: vec![],
                };
                let _ = store.record_interaction(&rec);
                continue;
            }

            let req = request_for(case);
            let started = Instant::now();
            let outcome = backend.translate(&req);
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            latencies.push(ms);
            match outcome {
                Ok(resp) => {
                    tokens_total += resp.usage.prompt_tokens.unwrap_or(0) as u64;
                    tokens_total += resp.usage.completion_tokens.unwrap_or(0) as u64;
                    let command = resp
                        .candidates
                        .first()
                        .map(|c| c.command.trim().to_string())
                        .unwrap_or_default();
                    if command.is_empty() {
                        if pass == 1 {
                            error_ids.push(case.id.clone());
                            rows.push(Row {
                                id: case.id.clone(),
                                tier: Tier::Error,
                                ms,
                                cached: false,
                                command: "empty command".to_string(),
                            });
                        }
                        continue;
                    }
                    let risk = RiskClassifier.assess(&command);
                    ts += 1;
                    let rec = Interaction {
                        id: None,
                        ts,
                        session_id: "eval".into(),
                        project_id: None,
                        cwd: case.cwd.clone(),
                        os: case.os.clone(),
                        shell: case.shell.clone(),
                        input_nl: case.intent.clone(),
                        output_cmd: command.clone(),
                        explanation: resp.candidates.first().and_then(|c| {
                            if c.explanation.is_empty() {
                                None
                            } else {
                                Some(c.explanation.clone())
                            }
                        }),
                        backend: resp.backend_id.clone(),
                        model: resp.model.clone(),
                        confidence: resp.confidence,
                        latency_ms: ms as u64,
                        risk_level: risk.level,
                        risk_notes: risk.rules.iter().map(|r| r.0.clone()).collect(),
                        from_cache: false,
                        accepted: None,
                        executed: None,
                        tags: vec![],
                    };
                    let _ = store.record_interaction(&rec);
                    if pass == 1 {
                        let tier = tier_of(case, &command);
                        match tier {
                            Tier::Exact => exact += 1,
                            Tier::Regex => regex_matched += 1,
                            Tier::Acceptable => acceptable += 1,
                            Tier::Miss | Tier::Error => {}
                        }
                        let reference = case
                            .expected
                            .reference()
                            .map_or(RiskLevel::Safe, |r| RiskClassifier.assess(r).level);
                        if is_risk_misclassified(risk.level, reference) {
                            risk_ids.push(case.id.clone());
                        }
                        rows.push(Row {
                            id: case.id.clone(),
                            tier,
                            ms,
                            cached: false,
                            command,
                        });
                    }
                }
                Err(err) => {
                    if pass == 1 {
                        error_ids.push(case.id.clone());
                        rows.push(Row {
                            id: case.id.clone(),
                            tier: Tier::Error,
                            ms,
                            cached: false,
                            command: err.to_string(),
                        });
                    }
                }
            }
        }
    }

    let n = cases.len();
    let mut sorted = latencies.clone();
    sorted.sort_by(f64::total_cmp);
    Report {
        metrics: Metrics {
            version: METRICS_VERSION,
            backend: backend.id().to_string(),
            model: model.to_string(),
            n_cases: n,
            passes,
            exact,
            regex_matched,
            acceptable,
            exact_rate: rate(exact, n),
            regex_rate: rate(regex_matched, n),
            acceptable_rate: rate(acceptable, n),
            risk_misclassified: risk_ids.len(),
            risk_ids,
            errors: error_ids.len(),
            error_ids,
            latency_p50_ms: percentile(&sorted, 50.0),
            latency_p95_ms: percentile(&sorted, 95.0),
            tokens_total,
            cache_lookups,
            cache_hits,
            cache_hit_rate: rate(cache_hits, cache_lookups),
        },
        rows,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_llm::{ErrorKind, MockBackend};
    use std::path::PathBuf;

    fn shipped_fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("fixtures/translate.json")
    }

    fn synthetic(intent: &str, expected: &str) -> Case {
        Case {
            id: format!("t-{intent}"),
            intent: intent.into(),
            os: "macos".into(),
            shell: "zsh".into(),
            cwd: "/tmp".into(),
            profile_name: "default".into(),
            ports: Vec::new(),
            prefer_docker: false,
            notes: String::new(),
            expected: crate::scoring::Expected::One(expected.into()),
        }
    }

    #[test]
    fn shipped_fixtures_parse_and_count_20() {
        let cases = load_cases(&shipped_fixtures()).expect("shipped fixtures parse");
        assert!(cases.len() >= 20, "want ≥ 20, got {}", cases.len());
        let mut ids = BTreeSet::new();
        for case in &cases {
            assert!(!case.intent.trim().is_empty(), "{}", case.id);
            assert!(ids.insert(case.id.clone()), "duplicate {}", case.id);
        }
    }

    #[test]
    fn offline_three_pass_cache_hits_on_third() {
        let cases = vec![
            synthetic("show current git status", "git status"),
            synthetic("list all files including hidden ones", "ls -la"),
        ];
        let mut backend = MockBackend::empty();
        backend.script_ok("show current git status", "git status", "status", 0.9);
        backend.script_ok(
            "list all files including hidden ones",
            "ls -la",
            "list",
            0.9,
        );
        let report = run_eval(&cases, &backend, "fixture", 3);
        assert_eq!(report.metrics.n_cases, 2);
        assert_eq!(report.metrics.errors, 0);
        assert_eq!(report.metrics.exact, 2);
        assert_eq!(report.metrics.exact_rate, 1.0);
        // Passes 2–3 look up twice per case; only pass 3 is eligible (2 priors).
        assert_eq!(report.metrics.cache_lookups, 4);
        assert_eq!(report.metrics.cache_hits, 2);
        assert_eq!(report.metrics.cache_hit_rate, 0.5);
        assert!(report.rows.iter().all(|r| r.cached));
        assert!(report.rows.iter().all(|r| r.tier == Tier::Exact));
    }

    #[test]
    fn backend_error_counts_and_continues() {
        let cases = vec![
            synthetic("show current git status", "git status"),
            synthetic("explode everything", "true"),
        ];
        let mut backend = MockBackend::empty();
        backend.script_ok("show current git status", "git status", "status", 0.9);
        backend.script_fault("explode everything", ErrorKind::Timeout);
        let report = run_eval(&cases, &backend, "fixture", 1);
        assert_eq!(report.metrics.errors, 1);
        assert_eq!(report.metrics.error_ids.len(), 1);
        assert!(report.metrics.error_ids[0].contains("explode"));
        assert_eq!(report.metrics.exact, 1);
        // The failing case is still reported as a row.
        assert_eq!(report.rows.len(), 2);
        assert!(report.rows.iter().any(|r| r.tier == Tier::Error));
    }

    #[test]
    fn danger_escalation_is_flagged_safe_fallback_is_not() {
        let cases = vec![
            synthetic("show current git status", "git status"),
            synthetic("list files", "ls"),
        ];
        let mut backend = MockBackend::empty();
        // Produced Danger vs Safe reference: misclassified.
        backend.script_ok("show current git status", "rm -rf /", "wipe", 0.9);
        // Produced Safe vs Safe reference: fine.
        backend.script_ok("list files", "ls -la", "list", 0.9);
        let report = run_eval(&cases, &backend, "fixture", 1);
        assert_eq!(report.metrics.risk_misclassified, 1);
        assert_eq!(report.metrics.risk_ids.len(), 1);

        // Safe fallback for a Danger reference is a coverage artifact, not a
        // classifier error: never flagged.
        let danger_ref = vec![synthetic("be careful", "rm -rf /")];
        let plain = MockBackend::empty();
        let quiet = run_eval(&danger_ref, &plain, "fixture", 1);
        assert_eq!(quiet.metrics.risk_misclassified, 0);
    }

    #[test]
    fn duplicate_ids_rejected() {
        let dir = std::env::temp_dir().join(format!(
            "shx-eval-dup-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("dup.json");
        std::fs::write(
            &path,
            r#"[{"id":"same","intent":"a","expected_command":"a"},{"id":"same","intent":"b","expected_command":"b"}]"#,
        )
        .expect("write");
        let err = load_cases(&path).expect_err("duplicates");
        assert!(err.to_string().contains("duplicate"), "{err}");
    }

    #[test]
    fn empty_and_invalid_fixtures_error() {
        let dir = std::env::temp_dir().join(format!(
            "shx-eval-bad-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("time")
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let empty = dir.join("empty.json");
        std::fs::write(&empty, "[]").expect("write");
        assert!(load_cases(&empty).is_err());

        let missing = dir.join("missing.json");
        std::fs::write(
            &missing,
            r#"[{"id":"x","intent":"  ","expected_command":"a"}]"#,
        )
        .expect("write");
        let err = load_cases(&missing).expect_err("blank intent");
        assert!(err.to_string().contains("intent"), "{err}");

        let no_expected = dir.join("noexp.json");
        std::fs::write(
            &no_expected,
            r#"[{"id":"x","intent":"a","expected_command":[]}]"#,
        )
        .expect("write");
        let err = load_cases(&no_expected).expect_err("empty expected");
        assert!(err.to_string().contains("expected_command"), "{err}");

        let missing_file = dir.join("does-not-exist.json");
        assert!(load_cases(&missing_file).is_err());
    }

    #[test]
    fn single_pass_has_no_cache_lookups() {
        let cases = vec![synthetic("show current git status", "git status")];
        let mut backend = MockBackend::empty();
        backend.script_ok("show current git status", "git status", "status", 0.9);
        let report = run_eval(&cases, &backend, "fixture", 1);
        assert_eq!(report.metrics.cache_lookups, 0);
        assert_eq!(report.metrics.cache_hits, 0);
        assert_eq!(report.metrics.cache_hit_rate, 0.0);
        assert!(!report.rows[0].cached);
    }
}
