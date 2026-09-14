//! Typed config mirroring docs/05-CLI-SPEC.md §4.

use serde::{Deserialize, Serialize};

/// Fully resolved configuration after layered merge.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct Config {
    /// Backend routing and provider settings.
    pub backend: BackendConfig,
    /// Memory store settings.
    pub memory: MemoryConfig,
    /// Risk banners and refuse policy.
    pub safety: SafetyConfig,
    /// Host / profile facts injected into prompts.
    pub context: ContextConfig,
    /// Human-facing UI knobs.
    pub ui: UiConfig,
    /// Snippet settings.
    pub snippets: SnippetsConfig,
}

/// `[backend]` table.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BackendConfig {
    /// `local` | `cloud` | `local-first`.
    pub mode: BackendMode,
    /// Escalate when confidence is below this value.
    pub escalate_below_confidence: f32,
    /// Escalate when local latency exceeds this, even if it succeeded.
    pub local_slow_ms: u64,
    /// Skip local on obviously multi-clause intents (ADR-009, default off).
    pub complexity_skip_local: bool,
    /// Local provider.
    pub local: LocalBackendConfig,
    /// Cloud provider.
    pub cloud: CloudBackendConfig,
}

impl Default for BackendConfig {
    fn default() -> Self {
        Self {
            mode: BackendMode::LocalFirst,
            escalate_below_confidence: 0.6,
            local_slow_ms: 8_000,
            complexity_skip_local: false,
            local: LocalBackendConfig::default(),
            cloud: CloudBackendConfig::default(),
        }
    }
}

/// Routing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum BackendMode {
    /// Never leave the machine.
    Local,
    /// Cloud only.
    Cloud,
    /// Local first, escalate on failure / low confidence.
    LocalFirst,
}

impl BackendMode {
    /// Parse a config string.
    pub fn from_str_cfg(s: &str) -> Option<Self> {
        match s {
            "local" => Some(Self::Local),
            "cloud" => Some(Self::Cloud),
            "local-first" => Some(Self::LocalFirst),
            _ => None,
        }
    }

    /// Values accepted in config / env / flags.
    pub fn accepted() -> &'static str {
        "local, cloud, local-first"
    }

    /// Canonical config spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
            Self::LocalFirst => "local-first",
        }
    }
}

/// `[backend.local]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LocalBackendConfig {
    /// Provider kind (`ollama`).
    pub kind: String,
    /// Chat endpoint.
    pub base_url: String,
    /// Model id.
    pub model: String,
    /// Ollama keep_alive duration.
    pub keep_alive: String,
    /// Requested context length.
    pub num_ctx: u32,
    /// Per-attempt timeout.
    pub timeout_ms: u64,
}

impl Default for LocalBackendConfig {
    fn default() -> Self {
        Self {
            kind: "ollama".into(),
            base_url: "http://127.0.0.1:11434".into(),
            model: "qwen3:14b".into(),
            keep_alive: "30m".into(),
            num_ctx: 4096,
            timeout_ms: 8_000,
        }
    }
}

/// `[backend.cloud]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CloudBackendConfig {
    /// `anthropic` | `openai-compat`.
    pub kind: String,
    /// Model id.
    pub model: String,
    /// Name of the env var that holds the key — never the key itself.
    pub api_key_env: String,
    /// Per-attempt timeout.
    pub timeout_ms: u64,
    /// Optional base URL (openai-compat).
    pub base_url: Option<String>,
}

impl Default for CloudBackendConfig {
    fn default() -> Self {
        Self {
            kind: "anthropic".into(),
            model: "claude-sonnet-4-5".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
            timeout_ms: 20_000,
            base_url: None,
        }
    }
}

/// `[memory]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryConfig {
    /// When false, skip read and write.
    pub enabled: bool,
    /// Empty string means the OS default location.
    pub path: String,
    /// Drop interactions older than this many days.
    pub retention_days: u32,
    /// Context assembly budget.
    pub context: MemoryContextBudget,
    /// Opt-in shell-history ingest.
    pub ingest_shell_history: bool,
    /// Redact secrets at write. Do not set this to false.
    pub redact_secrets: bool,
}

impl Default for MemoryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: String::new(),
            retention_days: 180,
            context: MemoryContextBudget::default(),
            ingest_shell_history: false,
            redact_secrets: true,
        }
    }
}

/// `[memory.context]` / inline `context = { … }`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MemoryContextBudget {
    /// Anchor recency window.
    pub recent: u32,
    /// Relevance hits.
    pub relevance: u32,
    /// Nearby shell entries.
    pub shell: u32,
    /// Hard cap on memory tokens in the prompt.
    pub max_tokens: u32,
}

impl Default for MemoryContextBudget {
    fn default() -> Self {
        Self {
            recent: 10,
            relevance: 5,
            shell: 5,
            max_tokens: 1500,
        }
    }
}

/// `[safety]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SafetyConfig {
    /// Print risk banners on stderr.
    pub warn_on_risk: bool,
    /// Exit 3 when risk ≥ Review.
    pub exit_on_risk: bool,
    /// Refuse multi-command output when risk is elevated.
    pub refuse_multi_command_on_risk: bool,
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self {
            warn_on_risk: true,
            exit_on_risk: false,
            refuse_multi_command_on_risk: true,
        }
    }
}

/// `[context]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ContextConfig {
    /// `auto` | `macos` | `linux` | `windows`.
    pub os: String,
    /// `auto` or a shell name.
    pub shell: String,
    /// `auto` | `true` | `false`.
    pub in_container: String,
    /// Prefer Docker when the intent is ambiguous.
    pub prefer_docker: bool,
    /// Ports the user actually uses.
    pub ports: Vec<u16>,
    /// Freeform notes injected into PROFILE.
    pub notes: String,
}

impl Default for ContextConfig {
    fn default() -> Self {
        Self {
            os: "auto".into(),
            shell: "auto".into(),
            in_container: "auto".into(),
            prefer_docker: true,
            ports: vec![3000, 5432, 7000],
            notes: String::new(),
        }
    }
}

/// `[ui]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UiConfig {
    /// `auto` | `always` | `never`.
    pub color: ColorMode,
    /// Echo latency on stderr.
    pub timing: bool,
    /// Update check policy (`never` in v1).
    pub update_check: String,
    /// Default `-n` / candidate count.
    pub candidates: u8,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            color: ColorMode::Auto,
            timing: false,
            update_check: "never".into(),
            candidates: 1,
        }
    }
}

/// Color output policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorMode {
    /// TTY + `NO_COLOR` / `CLICOLOR_FORCE`.
    Auto,
    /// Always emit ANSI.
    Always,
    /// Never emit ANSI.
    Never,
}

impl ColorMode {
    /// Parse a config string.
    pub fn from_str_cfg(s: &str) -> Option<Self> {
        match s {
            "auto" => Some(Self::Auto),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            _ => None,
        }
    }

    /// Values accepted in config / env / flags.
    pub fn accepted() -> &'static str {
        "auto, always, never"
    }
}

/// `[snippets]`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SnippetsConfig {
    /// P2: git repo of shared snippets.
    pub remote: Option<String>,
}

/// CLI flag overlay (highest precedence).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FlagOverrides {
    /// `--local` / `--cloud`.
    pub backend_mode: Option<BackendMode>,
    /// `--exit-on-risk`.
    pub exit_on_risk: Option<bool>,
    /// `--no-memory`.
    pub no_memory: Option<bool>,
    /// `-n` / `--count`.
    pub candidates: Option<u8>,
    /// `--no-color` / `--color`.
    pub color: Option<ColorMode>,
    /// `--config <path>` — not merged into values; used by discovery.
    pub config_path: Option<String>,
}
