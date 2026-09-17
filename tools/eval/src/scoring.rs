//! Fixture schema, scoring tiers, and metric aggregation.
//!
//! Pure functions only: no IO, no network, no clock. Every function here is
//! deterministic, so the offline report is byte-stable except for latency.

use serde::{Deserialize, Serialize};
use shx_core::RiskLevel;

/// Metrics contract version (bumped on breaking [`Metrics`] changes).
pub const METRICS_VERSION: u32 = 1;

/// One fixture case as stored in `translate.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct RawFixture {
    /// Stable case id (`case-N` when omitted, mirroring the T5 harness).
    #[serde(default)]
    pub id: Option<String>,
    /// Natural-language intent to translate.
    pub intent: String,
    /// Environment the intent was written for.
    #[serde(default)]
    pub env: RawEnv,
    /// Context profile the intent was written for.
    #[serde(default)]
    pub profile: RawProfile,
    /// Expected command: glob string (`*` wildcard) or alternatives (OR).
    /// `expected_command_regex_or_set` is accepted as an alias.
    #[serde(alias = "expected_command_regex_or_set")]
    pub expected_command: Expected,
}

/// Environment block of a fixture case.
#[derive(Debug, Clone, Deserialize)]
pub struct RawEnv {
    /// OS label (`macos` | `linux` | `windows`).
    #[serde(default = "default_os")]
    pub os: String,
    /// Shell label (`zsh` | `bash` | …).
    #[serde(default = "default_shell")]
    pub shell: String,
    /// Working directory.
    #[serde(default = "default_cwd")]
    pub cwd: String,
}

impl Default for RawEnv {
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

/// Profile block of a fixture case.
#[derive(Debug, Clone, Deserialize)]
pub struct RawProfile {
    /// Profile name.
    #[serde(default = "default_profile_name")]
    pub name: String,
    /// Ports the user actually uses.
    #[serde(default)]
    pub ports: Vec<u16>,
    /// Prefer Docker when the intent is ambiguous.
    #[serde(default)]
    pub prefer_docker: bool,
    /// Freeform notes.
    #[serde(default)]
    pub notes: String,
}

impl Default for RawProfile {
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

/// Expected command: one glob string or a set of alternatives (OR).
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum Expected {
    /// Single glob pattern.
    One(String),
    /// Alternatives; a command matching any of them scores.
    Any(Vec<String>),
}

impl Expected {
    /// All non-empty alternatives in order (first = canonical reference).
    pub fn alternatives(&self) -> Vec<&str> {
        match self {
            Self::One(s) if !s.trim().is_empty() => vec![s.as_str()],
            Self::One(_) => Vec::new(),
            Self::Any(v) => v
                .iter()
                .map(String::as_str)
                .filter(|s| !s.trim().is_empty())
                .collect(),
        }
    }

    /// True when no usable alternative exists (schema error when scoring).
    pub fn is_empty(&self) -> bool {
        self.alternatives().is_empty()
    }

    /// Canonical reference: the first alternative (used for the acceptable
    /// tier and the risk comparison).
    pub fn reference(&self) -> Option<&str> {
        self.alternatives().into_iter().next()
    }
}

/// `*` matches any sequence (including empty). Parts must appear in order;
/// a leading part anchors at the start unless the pattern starts with `*`,
/// and a trailing part anchors at the end unless the pattern ends with `*`.
/// Matching is case-sensitive and byte-safe.
pub fn glob_match(pattern: &str, text: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return text.contains(pattern);
    }
    let anchored_start = !pattern.starts_with('*');
    let anchored_end = !pattern.ends_with('*');
    let mut start = 0usize;
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        let is_first = i == 0 && anchored_start;
        let is_last = i == parts.len() - 1 && anchored_end;
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

/// Regex tier: `*` patterns use [`glob_match`]; a pattern without `*` scores
/// when it equals the trimmed command or appears inside it. Empty commands
/// and empty patterns never match. Same contract as the T5 live harness.
pub fn pattern_matches(pattern: &str, command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() || pattern.trim().is_empty() {
        return false;
    }
    if pattern.contains('*') {
        glob_match(pattern, cmd)
    } else {
        cmd == pattern || cmd.contains(pattern)
    }
}

/// Exact tier: the trimmed command equals any alternative exactly.
pub fn exact_match(expected: &Expected, command: &str) -> bool {
    let cmd = command.trim();
    if cmd.is_empty() {
        return false;
    }
    expected.alternatives().iter().any(|alt| cmd == alt.trim())
}

/// Regex tier: the command matches any alternative via [`pattern_matches`].
pub fn regex_match(expected: &Expected, command: &str) -> bool {
    expected
        .alternatives()
        .iter()
        .any(|alt| pattern_matches(alt, command))
}

/// First whitespace-separated token, lowercased (`None` when blank).
pub fn leading_token(command: &str) -> Option<String> {
    command
        .split_whitespace()
        .next()
        .map(|t| t.to_ascii_lowercase())
}

/// Acceptable tier: the produced command invokes the same program as the
/// canonical reference (equal lowercased leading tokens). Catches answers
/// that are plausible but flag- or path-different from every alternative.
pub fn acceptable_match(expected: &Expected, command: &str) -> bool {
    match (
        expected.reference().and_then(leading_token),
        leading_token(command),
    ) {
        (Some(a), Some(b)) => a == b,
        _ => false,
    }
}

/// Risk misclassification: the produced command is `Danger` while the
/// canonical reference is not. Flags answers that escalate destructiveness
/// beyond the fixture — the one risk regression the harness gates. A `Safe`
/// fallback for a `Danger` reference is a backend-coverage artifact, not a
/// classifier error, so it does not count.
pub fn is_risk_misclassified(produced: RiskLevel, reference: RiskLevel) -> bool {
    produced == RiskLevel::Danger && reference != RiskLevel::Danger
}

/// Nearest-rank percentile over a sorted slice (`pct` in 0.0–100.0).
/// Empty input yields 0.0; out-of-range `pct` is clamped.
pub fn percentile(sorted: &[f64], pct: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let pct = pct.clamp(0.0, 100.0);
    let rank = (pct / 100.0 * sorted.len() as f64).ceil() as usize;
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

/// Ratio in 0.0–1.0; `0/0` is defined as 0.0 (e.g. cache rate with one pass).
pub fn rate(part: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 / total as f64
    }
}

/// Aggregate report numbers. Tiers are independent checks (exact implies
/// regex only when the matched alternative has no wildcard), each reported
/// as `count/n`.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Metrics {
    /// [`METRICS_VERSION`].
    pub version: u32,
    /// Backend that translated (`mock` offline, `ollama` live).
    pub backend: String,
    /// Model that translated (`fixture` for the mock).
    pub model: String,
    /// Fixture cases scored.
    pub n_cases: usize,
    /// Repeat passes (pass 1 scores quality; later passes score cache).
    pub passes: usize,
    /// Exact-tier hits.
    pub exact: usize,
    /// Regex-tier hits.
    pub regex_matched: usize,
    /// Acceptable-tier hits.
    pub acceptable: usize,
    /// `exact / n_cases`.
    pub exact_rate: f64,
    /// `regex_matched / n_cases`.
    pub regex_rate: f64,
    /// `acceptable / n_cases`.
    pub acceptable_rate: f64,
    /// Danger-escalations beyond the reference (see [`is_risk_misclassified`]).
    pub risk_misclassified: usize,
    /// Ids flagged by the risk check, in fixture order.
    pub risk_ids: Vec<String>,
    /// Cases that errored or came back empty (pass 1).
    pub errors: usize,
    /// Ids that errored, in fixture order.
    pub error_ids: Vec<String>,
    /// p50 serve latency in milliseconds.
    pub latency_p50_ms: f64,
    /// p95 serve latency in milliseconds.
    pub latency_p95_ms: f64,
    /// Summed prompt + completion tokens over translate calls (`0` for mock).
    pub tokens_total: u64,
    /// Cache lookups on repeat passes.
    pub cache_lookups: usize,
    /// Cache hits on repeat passes.
    pub cache_hits: usize,
    /// `cache_hits / cache_lookups`.
    pub cache_hit_rate: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn one(pat: &str) -> Expected {
        Expected::One(pat.into())
    }

    fn any(pats: &[&str]) -> Expected {
        Expected::Any(pats.iter().map(|s| (*s).into()).collect())
    }

    #[test]
    fn glob_star_matches_in_order() {
        assert!(glob_match(
            "docker run*7000*postgres*",
            "docker run --name pg -p 7000:5432 -d postgres:16"
        ));
        assert!(!glob_match(
            "docker run*7000*postgres*",
            "podman run --name pg -p 7000:5432 -d postgres:16"
        ));
        assert!(!glob_match(
            "docker run*7000*postgres*",
            "docker run --name pg -p 7000:5432 -d redis:7"
        ));
        // Reversed part order must not match.
        assert!(!glob_match("a*b", "b-a"));
        assert!(glob_match("a*b", "a-b"));
    }

    #[test]
    fn glob_anchoring_and_empty_edges() {
        // No leading `*`: first part anchors at the start.
        assert!(glob_match("git*", "git status"));
        assert!(!glob_match("git*", "xgit status"));
        // No trailing `*`: last part anchors at the end.
        assert!(glob_match("*status", "git status"));
        assert!(!glob_match("*status", "git status -sb"));
        // Both ends open.
        assert!(glob_match("*status*", "git status -sb"));
        // Bare `*` matches any text at the glob level; the tier layer
        // (`pattern_matches`) still rejects empty commands.
        assert!(glob_match("*", "anything"));
        assert!(glob_match("*", ""));
        assert!(!pattern_matches("*", ""));
        // Adjacent stars behave like one.
        assert!(glob_match("a**b", "a-b"));
        // Unicode is byte-safe (multi-byte chars around and inside matches).
        assert!(glob_match("café*au lait", "café au lait"));
    }

    #[test]
    fn no_star_is_substring_or_exact() {
        assert!(pattern_matches("git status", "git status"));
        assert!(pattern_matches("git status", "git status -sb"));
        assert!(!pattern_matches("git status", "gitstatus"));
        assert!(!pattern_matches("", "git status"));
        assert!(!pattern_matches("git status", ""));
        assert!(!pattern_matches("", ""));
        assert!(!pattern_matches("   ", "git status"));
    }

    #[test]
    fn tiers_exact_regex_acceptable() {
        let exp = any(&["git reset --soft HEAD~1", "git reset --soft HEAD^"]);
        assert!(exact_match(&exp, "git reset --soft HEAD~1"));
        assert!(exact_match(&exp, "  git reset --soft HEAD~1  "));
        assert!(!exact_match(&exp, "git reset --soft HEAD~1 --force"));
        assert!(regex_match(&exp, "git reset --soft HEAD~1"));
        assert!(acceptable_match(&exp, "git reset --hard HEAD~1"));
        assert!(!acceptable_match(&exp, "svn revert ."));

        let glob = one("docker run*7000*postgres*");
        let cmd = "docker run --name pg -p 7000:5432 -d postgres:16";
        assert!(!exact_match(&glob, cmd));
        assert!(regex_match(&glob, cmd));
        assert!(acceptable_match(&glob, cmd));
        // Same program, different flags: acceptable but not regex.
        assert!(!regex_match(&glob, "docker ps"));
        assert!(acceptable_match(&glob, "docker ps"));
    }

    #[test]
    fn acceptable_uses_leading_token_case_insensitive() {
        let exp = one("tmux attach");
        assert!(acceptable_match(&exp, "TMUX attach -t main"));
        assert!(!acceptable_match(&exp, ""));
        assert!(!acceptable_match(&one(""), "tmux attach"));
        assert!(!acceptable_match(&exp, "   "));
    }

    #[test]
    fn risk_danger_escalation_only() {
        use RiskLevel::{Danger, Review, Safe};
        assert!(is_risk_misclassified(Danger, Safe));
        assert!(is_risk_misclassified(Danger, Review));
        assert!(!is_risk_misclassified(Danger, Danger));
        assert!(!is_risk_misclassified(Safe, Danger));
        assert!(!is_risk_misclassified(Review, Danger));
        assert!(!is_risk_misclassified(Safe, Safe));
    }

    #[test]
    fn percentiles_nearest_rank() {
        let mut v = vec![4.0, 1.0, 3.0, 2.0];
        v.sort_by(f64::total_cmp);
        assert_eq!(percentile(&v, 50.0), 2.0);
        assert_eq!(percentile(&v, 95.0), 4.0);
        assert_eq!(percentile(&v, 0.0), 1.0);
        assert_eq!(percentile(&v, 100.0), 4.0);
        assert_eq!(percentile(&v, 25.0), 1.0);
        assert_eq!(percentile(&[], 50.0), 0.0);
        assert_eq!(percentile(&v, 1000.0), 4.0);
        let single = vec![7.5];
        assert_eq!(percentile(&single, 50.0), 7.5);
    }

    #[test]
    fn rate_handles_zero_total() {
        assert_eq!(rate(0, 0), 0.0);
        assert_eq!(rate(0, 4), 0.0);
        assert_eq!(rate(3, 4), 0.75);
    }

    #[test]
    fn fixture_schema_string_array_alias() {
        let one: RawFixture =
            serde_json::from_str(r#"{"id":"a","intent":"x","expected_command":"git status"}"#)
                .expect("string form");
        assert_eq!(one.id.as_deref(), Some("a"));
        assert!(!one.expected_command.is_empty());

        let many: RawFixture =
            serde_json::from_str(r#"{"intent":"x","expected_command":["a","b"]}"#)
                .expect("array form");
        assert!(many.id.is_none());
        assert_eq!(many.expected_command.alternatives(), vec!["a", "b"]);

        let alias: RawFixture =
            serde_json::from_str(r#"{"intent":"x","expected_command_regex_or_set":["a"]}"#)
                .expect("alias form");
        assert_eq!(alias.expected_command.alternatives(), vec!["a"]);

        let empty: RawFixture =
            serde_json::from_str(r#"{"intent":"x","expected_command":[]}"#).expect("parses");
        assert!(empty.expected_command.is_empty());
        let blanks: RawFixture =
            serde_json::from_str(r#"{"intent":"x","expected_command":["  ",""]}"#).expect("parses");
        assert!(blanks.expected_command.is_empty());
        // Env/profile defaults apply when omitted.
        assert_eq!(many.env.os, "macos");
        assert_eq!(many.env.shell, "zsh");
        assert_eq!(many.env.cwd, "/tmp");
        assert_eq!(many.profile.name, "default");
    }
}
