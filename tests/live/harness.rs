//! T5 live harness (docs/08-TESTING.md).
//!
//! Offline: fixture seed tests always run (no network).
//! Live: `SHX_LIVE=1 cargo test -p shx-llm --test live -- --ignored --nocapture`
//!
//! Print-only: this harness translates intents; it never executes the result.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

use serde::Deserialize;
use shx_core::{ContextBundle, EnvInfo, Intent, IntentFlags, NoopRedactor, Profile, PromptBuilder};
use shx_llm::{Backend, OllamaBackend, OllamaSettings};

const FIXTURES_REL: &str = "tools/eval/fixtures/translate.json";
const MIN_CASES: usize = 20;
const DEFAULT_LIVE_TIMEOUT_MS: u64 = 60_000;

#[derive(Debug, Deserialize)]
struct Fixture {
    #[serde(default)]
    id: Option<String>,
    intent: String,
    #[serde(default)]
    env: FixtureEnv,
    #[serde(default)]
    profile: FixtureProfile,
    #[serde(alias = "expected_command_regex_or_set")]
    expected_command: ExpectedCommand,
}

#[derive(Debug, Deserialize)]
struct FixtureEnv {
    #[serde(default = "default_os")]
    os: String,
    #[serde(default = "default_shell")]
    shell: String,
    #[serde(default = "default_cwd")]
    cwd: String,
}

impl Default for FixtureEnv {
    fn default() -> Self {
        Self {
            os: default_os(),
            shell: default_shell(),
            cwd: default_cwd(),
        }
    }
}

fn default_os() -> String {
    "macos".into()
}
fn default_shell() -> String {
    "zsh".into()
}
fn default_cwd() -> String {
    "/tmp".into()
}

#[derive(Debug, Deserialize)]
struct FixtureProfile {
    #[serde(default = "default_profile_name")]
    name: String,
    #[serde(default)]
    ports: Vec<u16>,
    #[serde(default)]
    prefer_docker: bool,
    #[serde(default)]
    notes: String,
}

impl Default for FixtureProfile {
    fn default() -> Self {
        Self {
            name: default_profile_name(),
            ports: Vec::new(),
            prefer_docker: false,
            notes: String::new(),
        }
    }
}

fn default_profile_name() -> String {
    "default".into()
}

/// Glob string (`*` wildcard) or a set of alternatives (OR).
#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ExpectedCommand {
    One(String),
    Any(Vec<String>),
}

impl ExpectedCommand {
    fn matches(&self, command: &str) -> bool {
        match self {
            Self::One(pat) => pattern_matches(pat, command),
            Self::Any(pats) => pats.iter().any(|pat| pattern_matches(pat, command)),
        }
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::One(s) => s.is_empty(),
            Self::Any(v) => v.is_empty() || v.iter().all(|s| s.is_empty()),
        }
    }
}

/// `*` matches any sequence (including empty). A pattern with no `*` is a
/// substring (or exact) match against the trimmed command.
fn pattern_matches(pat: &str, command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() || pat.is_empty() {
        return false;
    }
    if pat.contains('*') {
        glob_match(pat, cmd)
    } else {
        cmd == pat || cmd.contains(pat)
    }
}

fn glob_match(pat: &str, text: &str) -> bool {
    let parts: Vec<&str> = pat.split('*').collect();
    if parts.len() == 1 {
        return text.contains(pat);
    }
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let is_first = i == 0 && !pat.starts_with('*');
        let is_last = i == parts.len() - 1 && !pat.ends_with('*');
        if is_first {
            if !text[start..].starts_with(part) {
                return false;
            }
            start += part.len();
            continue;
        }
        if is_last {
            return text[start..].ends_with(part);
        }
        match text[start..].find(part) {
            Some(idx) => start += idx + part.len(),
            None => return false,
        }
    }
    true
}

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("workspace root")
}

fn fixtures_path() -> PathBuf {
    workspace_root().join(FIXTURES_REL)
}

fn load_fixtures(path: &Path) -> Vec<Fixture> {
    let raw = fs::read_to_string(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    serde_json::from_str(&raw).unwrap_or_else(|e| panic!("parse {}: {e}", path.display()))
}

fn live_enabled() -> bool {
    env::var("SHX_LIVE").ok().as_deref() == Some("1")
}

fn trunc(s: &str, max: usize) -> String {
    let mut chars = s.chars();
    let mut out: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        out = out.chars().take(max.saturating_sub(3)).collect();
        out.push_str("...");
    }
    out
}

struct Row {
    id: String,
    intent: String,
    verdict: &'static str,
    ms: u64,
    command: String,
}

fn print_table(rows: &[Row]) {
    eprintln!(
        "{:<18} {:<28} {:<6} {:>6}  command",
        "id", "intent", "match", "ms"
    );
    eprintln!("{}", "-".repeat(96));
    for row in rows {
        eprintln!(
            "{:<18} {:<28} {:<6} {:>6}  {}",
            trunc(&row.id, 18),
            trunc(&row.intent, 28),
            row.verdict,
            row.ms,
            trunc(&row.command, 40)
        );
    }
}

fn settings_from_env() -> OllamaSettings {
    let mut s = OllamaSettings {
        timeout_ms: env::var("SHX_LIVE_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_LIVE_TIMEOUT_MS),
        ..OllamaSettings::default()
    };
    if let Ok(model) = env::var("SHX_LIVE_MODEL")
        && !model.is_empty()
    {
        s.model = model;
    }
    if let Ok(url) = env::var("SHX_LIVE_URL")
        && !url.is_empty()
    {
        s.base_url = url;
    }
    s
}

fn request_for(fix: &Fixture) -> shx_core::TranslateRequest {
    let intent = Intent {
        text: fix.intent.clone(),
        force_backend: None,
        count: 1,
        flags: IntentFlags::default(),
    };
    let ctx = ContextBundle {
        env: EnvInfo {
            os: fix.env.os.clone(),
            shell: fix.env.shell.clone(),
            cwd: fix.env.cwd.clone(),
            git_root: None,
            in_container: false,
        },
        profile: Profile {
            name: fix.profile.name.clone(),
            ports: fix.profile.ports.clone(),
            prefer_docker: fix.profile.prefer_docker,
            notes: fix.profile.notes.clone(),
        },
        history: vec![],
        vocabulary: vec![],
        snippets: vec![],
        shell: vec![],
    };
    PromptBuilder.build(&intent, &ctx, &NoopRedactor)
}

fn case_id(fix: &Fixture, idx: usize) -> String {
    fix.id
        .clone()
        .unwrap_or_else(|| format!("case-{}", idx + 1))
}

/// Offline: the T-104 seed is present and well-formed so T6 has something to score.
#[test]
fn fixtures_seed_has_at_least_20_intents() {
    let cases = load_fixtures(&fixtures_path());
    assert!(
        cases.len() >= MIN_CASES,
        "{}: want ≥ {MIN_CASES} intents, got {}",
        FIXTURES_REL,
        cases.len()
    );
}

#[test]
fn fixture_cases_have_intent_env_profile_and_expected() {
    let cases = load_fixtures(&fixtures_path());
    assert!(!cases.is_empty(), "fixtures must not be empty");
    for (i, fix) in cases.iter().enumerate() {
        assert!(
            !fix.intent.trim().is_empty(),
            "case {i}: intent must be non-empty"
        );
        assert!(
            !fix.env.os.is_empty() && !fix.env.shell.is_empty() && !fix.env.cwd.is_empty(),
            "case {i} ({}): env.os/shell/cwd required",
            case_id(fix, i)
        );
        assert!(
            !fix.profile.name.is_empty(),
            "case {i}: profile.name required"
        );
        assert!(
            !fix.expected_command.is_empty(),
            "case {i}: expected_command must not be empty"
        );
    }
}

#[test]
fn glob_match_requires_parts_in_order() {
    assert!(glob_match(
        "docker run*7000*postgres*",
        "docker run --name pg -p 7000:5432 -d postgres:16"
    ));
    assert!(!glob_match(
        "docker run*7000*postgres*",
        "podman run --name pg -p 7000:5432 -d postgres:16"
    ));
    assert!(pattern_matches("git status", "git status -sb"));
    assert!(
        ExpectedCommand::Any(vec!["tmux attach".into(), "tmux a".into()])
            .matches("tmux attach -t main")
    );
    assert!(!ExpectedCommand::One("ollama pull qwen3:14b".into()).matches(""));
}

/// Live translations against a running Ollama. Skipped unless `SHX_LIVE=1`
/// *and* `--ignored` (so `cargo xtask ci` never hits the network).
///
/// ```sh
/// SHX_LIVE=1 cargo test -p shx-llm --test live -- --ignored --nocapture
/// ```
#[test]
#[ignore = "T5 live harness: SHX_LIVE=1 cargo test -p shx-llm --test live -- --ignored --nocapture"]
fn live_harness() {
    if !live_enabled() {
        eprintln!("T5 skipped: set SHX_LIVE=1 to run against a live backend");
        return;
    }

    let cases = load_fixtures(&fixtures_path());
    assert!(
        cases.len() >= MIN_CASES,
        "live run needs the 20-intent seed, got {}",
        cases.len()
    );

    let settings = settings_from_env();
    let base_url = settings.base_url.clone();
    let model = settings.model.clone();
    let backend = OllamaBackend::new(settings);
    let health = backend.health();
    assert!(
        health.reachable,
        "start ollama at {base_url} (or set SHX_LIVE_URL)"
    );
    assert!(
        health.model_present,
        "{}",
        health
            .message
            .unwrap_or_else(|| format!("run: ollama pull {model}"))
    );

    let mut rows = Vec::with_capacity(cases.len());
    let mut errors = 0usize;
    let mut hits = 0usize;

    for (i, fix) in cases.iter().enumerate() {
        let id = case_id(fix, i);
        let req = request_for(fix);
        let started = Instant::now();
        match backend.translate(&req) {
            Ok(resp) => {
                let ms = started.elapsed().as_millis() as u64;
                let command = resp
                    .candidates
                    .first()
                    .map(|c| c.command.trim().to_string())
                    .unwrap_or_default();
                if command.is_empty() {
                    errors += 1;
                    rows.push(Row {
                        id,
                        intent: fix.intent.clone(),
                        verdict: "empty",
                        ms,
                        command: String::new(),
                    });
                    continue;
                }
                let hit = fix.expected_command.matches(&command);
                if hit {
                    hits += 1;
                }
                rows.push(Row {
                    id,
                    intent: fix.intent.clone(),
                    verdict: if hit { "ok" } else { "miss" },
                    ms,
                    command,
                });
            }
            Err(err) => {
                errors += 1;
                rows.push(Row {
                    id,
                    intent: fix.intent.clone(),
                    verdict: "error",
                    ms: started.elapsed().as_millis() as u64,
                    command: err.to_string(),
                });
            }
        }
    }

    print_table(&rows);
    eprintln!(
        "\n{hits}/{} matched expected, {} errors, {} translated (print-only; commands were not executed)",
        cases.len(),
        errors,
        cases.len() - errors
    );

    assert_eq!(
        errors, 0,
        "T5 live: {errors} translation error(s); see table above"
    );
}
