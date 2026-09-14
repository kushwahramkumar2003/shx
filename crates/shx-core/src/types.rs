//! Domain types shared across the workspace.
//!
//! These structs and enums are a frozen interface (ADR-011). Field or variant
//! changes require an ADR.

use serde::{Deserialize, Serialize};

/// Natural-language request after CLI parsing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    /// Joined intent text (`shx "run pg on 7000"` → `"run pg on 7000"`).
    pub text: String,
    /// `--local` / `--cloud` / an explicit backend id.
    pub force_backend: Option<ForceBackend>,
    /// `-n` / `--count`: how many candidates to return.
    pub count: u8,
    /// Invocation flags that affect the pipeline but not the prompt text.
    pub flags: IntentFlags,
}

/// Force a particular backend for one invocation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ForceBackend {
    /// `--local`: never escalate to cloud.
    Local,
    /// `--cloud`: skip local (error if unconfigured).
    Cloud,
    /// Named provider id (`"ollama"`, `"anthropic"`, `"mock"`, …).
    Named(String),
}

/// Boolean flags parsed from the CLI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct IntentFlags {
    /// `--json`: structured object on stdout.
    pub json: bool,
    /// `--why`: explain memory/routing on stderr.
    pub why: bool,
    /// `--offline`: mock/fixtures only; no network.
    pub offline: bool,
    /// `--no-memory`: skip read and write of the store.
    pub no_memory: bool,
    /// `--exit-on-risk`: exit 3 when risk ≥ Review.
    pub exit_on_risk: bool,
    /// `--copy`: copy the command (feature `clipboard`).
    pub copy: bool,
    /// `-q` / `--quiet`.
    pub quiet: bool,
    /// `-v` / `--verbose`.
    pub verbose: bool,
}

/// Token-budgeted context assembled for one translation (ADR-005).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ContextBundle {
    /// Host environment snapshot.
    pub env: EnvInfo,
    /// User profile (ports, docker preference, notes).
    pub profile: Profile,
    /// Recency + relevance interactions, already redacted and budgeted.
    pub history: Vec<Interaction>,
    /// Matching / high-weight shorthand.
    pub vocabulary: Vec<VocabEntry>,
    /// Named macros matching the intent.
    pub snippets: Vec<Snippet>,
    /// Nearby imported shell history (empty unless ingested).
    pub shell: Vec<ShellEntry>,
}

/// OS / shell / cwd facts injected into the prompt as ENVIRONMENT.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EnvInfo {
    /// `"macos"` | `"linux"` | `"windows"`.
    pub os: String,
    /// `"zsh"` | `"bash"` | `"fish"` | `"powershell"` | ….
    pub shell: String,
    /// Working directory of the invocation.
    pub cwd: String,
    /// Git root if inside a repo.
    pub git_root: Option<String>,
    /// True when running inside a container / codespace.
    pub in_container: bool,
}

/// Named context profile (`--profile`, `[context]` in config).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Profile {
    /// Profile name (`"default"` unless `--profile` is set).
    pub name: String,
    /// Ports the user actually uses; preferred over invented ones.
    pub ports: Vec<u16>,
    /// Prefer Docker over bare processes when the intent is ambiguous.
    pub prefer_docker: bool,
    /// Freeform notes injected into PROFILE.
    pub notes: String,
}

/// One translation the tool has already produced.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Interaction {
    /// Store-assigned id; `None` before insert.
    pub id: Option<i64>,
    /// Unix milliseconds, UTC.
    pub ts: i64,
    /// Per-process uuid grouping a refine/chat session.
    pub session_id: String,
    /// Hash of git root; `None` outside a repo.
    pub project_id: Option<String>,
    /// Working directory at translation time.
    pub cwd: String,
    /// Host OS label.
    pub os: String,
    /// Host shell label.
    pub shell: String,
    /// Redacted natural-language input.
    pub input_nl: String,
    /// Redacted resolved command.
    pub output_cmd: String,
    /// Model explanation, if any.
    pub explanation: Option<String>,
    /// Backend id (`"ollama"`, `"anthropic"`, …).
    pub backend: String,
    /// Model id as reported by the backend.
    pub model: String,
    /// 0.0–1.0, or `None` if the backend did not report one.
    pub confidence: Option<f32>,
    /// End-to-end translation latency.
    pub latency_ms: u64,
    /// Classifier result stored with the row.
    pub risk_level: RiskLevel,
    /// Rule ids (and optional notes) that fired.
    pub risk_notes: Vec<String>,
    /// True when served from the fast-path cache.
    pub from_cache: bool,
    /// `None` unknown, `Some(true)` chosen, `Some(false)` rejected.
    pub accepted: Option<bool>,
    /// Set only if a wrapper reports execution; the core never sets this.
    pub executed: Option<bool>,
    /// Freeform tags (`["database","docker"]`).
    pub tags: Vec<String>,
}

/// Memory scope selecting which rows a query sees (ADR-003).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Scope {
    /// Tool translations (default).
    Tool,
    /// Imported real shell history (opt-in).
    Shell,
    /// Tool translations filtered to one git root.
    Project {
        /// Git-root hash; `None` means "current project".
        id: Option<String>,
    },
}

/// Learned or taught shorthand (`pg` → `postgres`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VocabEntry {
    /// Normalized lowercase term.
    pub term: String,
    /// Expansion used in prompts.
    pub expansion: String,
    /// Confirmation weight (taught starts at 2.0; applied at ≥ 1.5).
    pub weight: f64,
    /// How the entry was created.
    pub source: VocabSource,
    /// Last-used unix millis.
    pub last_used_ts: i64,
    /// Times this expansion was applied.
    pub use_count: u64,
}

/// Origin of a vocabulary row.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum VocabSource {
    /// `shx teach`.
    Taught,
    /// Implicit learning from `good` feedback.
    Learned,
    /// Imported from elsewhere.
    Imported,
}

/// User-named command macro.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Snippet {
    /// Store-assigned id; `None` before insert.
    pub id: Option<i64>,
    /// Unique name (`"pg-up"`).
    pub name: String,
    /// Command text (redacted at write).
    pub command: String,
    /// Optional description used for matching.
    pub description: Option<String>,
    /// Created unix millis.
    pub created_ts: i64,
    /// Times this snippet was surfaced.
    pub use_count: u64,
}

/// One imported shell-history line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShellEntry {
    /// Unix millis.
    pub ts: i64,
    /// Directory if known.
    pub cwd: Option<String>,
    /// Redacted command text.
    pub cmd: String,
    /// Exit code if the importer had one.
    pub exit_code: Option<i32>,
}

/// One candidate command from a backend.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Candidate {
    /// The command to print on stdout.
    pub command: String,
    /// Human explanation (stderr / `--json` only).
    pub explanation: String,
    /// Model-reported or heuristic confidence, 0.0–1.0.
    pub confidence: f32,
}

/// Classifier output for one command. Never rewrites the command.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RiskAssessment {
    /// Highest level among matched rules (`Safe` if none).
    pub level: RiskLevel,
    /// Stable rule ids (`"del.recursive-force"`).
    pub rules: Vec<RuleId>,
    /// Human-readable notes shown on stderr.
    pub notes: Vec<String>,
}

/// Destructive-potential of a printed command.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RiskLevel {
    /// No rules matched.
    Safe,
    /// Plausible-but-careful (`git reset --hard`, broad `kill`).
    Review,
    /// Irreversible / high blast radius (`rm -rf /`, `dd`, pipe-to-shell).
    Danger,
}

/// Stable classifier rule identifier (`"del.recursive-force"`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RuleId(pub String);

impl RuleId {
    /// Construct from a static or owned id.
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for RuleId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

/// Request sent to a backend (fields from docs/06-BACKENDS.md §1).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslateRequest {
    /// System prompt (rules + schema reminder).
    pub system: String,
    /// User prompt (environment, profile, memory blocks, intent).
    pub user: String,
    /// JSON Schema the model must satisfy (ADR-007).
    pub schema: OutputSchema,
    /// Completion cap.
    pub max_tokens: u32,
    /// Sampling temperature (default 0.1).
    pub temperature: f32,
    /// Per-attempt timeout.
    pub timeout_ms: u64,
}

/// JSON Schema describing the model's required output contract (ADR-007).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputSchema {
    /// Schema name sent to providers that require one.
    pub name: String,
    /// JSON Schema object.
    pub json: serde_json::Value,
}

impl OutputSchema {
    /// Placeholder v1 translate schema. The full contract lands in T-101.
    pub fn translate_v1() -> Self {
        Self {
            name: "shx_translate".into(),
            json: serde_json::json!({
                "type": "object",
                "required": ["commands"],
                "properties": {
                    "commands": { "type": "array" },
                    "risk_notes": { "type": "array" },
                    "assumptions": { "type": "array" }
                }
            }),
        }
    }
}

/// Successful backend response after schema validation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TranslateResponse {
    /// One or more candidates; the pipeline picks the first unless `-n`.
    pub candidates: Vec<Candidate>,
    /// Raw model text for `--verbose`; never printed by default.
    pub raw: String,
    /// Token usage if the provider reported it.
    pub usage: Usage,
    /// Backend id (`"ollama"`, `"mock"`, …). Owned so the value is serde-friendly.
    pub backend_id: String,
    /// Model id as reported by the provider.
    pub model: String,
    /// Provider round-trip latency.
    pub latency_ms: u64,
    /// Backend self-report or heuristic; `None` if unknown.
    pub confidence: Option<f32>,
}

/// Token accounting from a provider. All fields optional.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Usage {
    /// Prompt / input tokens.
    pub prompt_tokens: Option<u32>,
    /// Completion / output tokens.
    pub completion_tokens: Option<u32>,
}

/// Feedback verdict stored against an interaction.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    /// User marked the translation good.
    Good,
    /// User marked the translation bad.
    Bad,
}

/// Retention knobs applied by `MemoryStore::prune`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrunePolicy {
    /// Drop tool interactions older than this many days.
    pub retention_days: u32,
    /// If false, strip `danger` commands after 30 days.
    pub keep_danger: bool,
}

/// Counts returned by a prune pass.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PruneReport {
    /// Interaction rows deleted.
    pub interactions_deleted: u64,
    /// Vocabulary rows whose weight was decayed.
    pub vocab_decayed: u64,
    /// Danger commands stripped (row kept, command redacted/cleared).
    pub danger_commands_stripped: u64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::de::DeserializeOwned;

    fn roundtrip<T>(value: &T)
    where
        T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug,
    {
        let json = serde_json::to_string(value).expect("serialize");
        let back: T = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(*value, back);
    }

    fn sample_interaction() -> Interaction {
        Interaction {
            id: Some(1),
            ts: 1_700_000_000_000,
            session_id: "sess".into(),
            project_id: Some("ab12".into()),
            cwd: "/tmp".into(),
            os: "macos".into(),
            shell: "zsh".into(),
            input_nl: "run pg on 7000".into(),
            output_cmd: "docker run --name pg -p 7000:5432 -d postgres:16".into(),
            explanation: Some("start postgres".into()),
            backend: "mock".into(),
            model: "fixture".into(),
            confidence: Some(0.9),
            latency_ms: 12,
            risk_level: RiskLevel::Safe,
            risk_notes: vec![],
            from_cache: false,
            accepted: None,
            executed: None,
            tags: vec!["docker".into()],
        }
    }

    #[test]
    fn serde_roundtrip_each_type() {
        roundtrip(&Intent {
            text: "run pg on 7000".into(),
            force_backend: Some(ForceBackend::Local),
            count: 1,
            flags: IntentFlags {
                json: true,
                ..IntentFlags::default()
            },
        });
        roundtrip(&ContextBundle {
            env: EnvInfo {
                os: "macos".into(),
                shell: "zsh".into(),
                cwd: "/tmp".into(),
                git_root: None,
                in_container: false,
            },
            profile: Profile {
                name: "default".into(),
                ports: vec![7000],
                prefer_docker: true,
                notes: String::new(),
            },
            history: vec![sample_interaction()],
            vocabulary: vec![VocabEntry {
                term: "pg".into(),
                expansion: "postgres".into(),
                weight: 2.0,
                source: VocabSource::Taught,
                last_used_ts: 1,
                use_count: 3,
            }],
            snippets: vec![Snippet {
                id: Some(1),
                name: "pg-up".into(),
                command: "docker run postgres".into(),
                description: None,
                created_ts: 1,
                use_count: 0,
            }],
            shell: vec![ShellEntry {
                ts: 1,
                cwd: Some("/tmp".into()),
                cmd: "ls".into(),
                exit_code: Some(0),
            }],
        });
        roundtrip(&Candidate {
            command: "ls".into(),
            explanation: "list".into(),
            confidence: 0.5,
        });
        roundtrip(&RiskAssessment {
            level: RiskLevel::Danger,
            rules: vec![RuleId::new("del.recursive-force")],
            notes: vec!["rm -rf /".into()],
        });
        roundtrip(&RiskLevel::Review);
        roundtrip(&TranslateRequest {
            system: "sys".into(),
            user: "user".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 256,
            temperature: 0.1,
            timeout_ms: 8_000,
        });
        roundtrip(&TranslateResponse {
            candidates: vec![Candidate {
                command: "ls".into(),
                explanation: "list".into(),
                confidence: 0.8,
            }],
            raw: "{}".into(),
            usage: Usage {
                prompt_tokens: Some(10),
                completion_tokens: Some(4),
            },
            backend_id: "mock".into(),
            model: "fixture".into(),
            latency_ms: 3,
            confidence: Some(0.8),
        });
        roundtrip(&Usage::default());
        roundtrip(&sample_interaction());
        roundtrip(&Scope::Project {
            id: Some("ab12".into()),
        });
        roundtrip(&Verdict::Good);
        roundtrip(&PrunePolicy {
            retention_days: 180,
            keep_danger: false,
        });
        roundtrip(&PruneReport::default());
    }
}
