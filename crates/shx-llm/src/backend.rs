//! Frozen [`Backend`] trait and supporting types (docs/02-ARCHITECTURE.md §4).
//!
//! v1 is synchronous (ADR-006). Streaming, if added, is an additive method
//! with a default impl that falls back to [`Backend::translate`].

use serde::{Deserialize, Serialize};
use shx_core::{TranslateRequest, TranslateResponse};

/// A model provider that turns a [`TranslateRequest`] into candidates.
pub trait Backend: Send + Sync {
    /// Stable id (`"ollama"`, `"anthropic"`, `"openai-compat"`, `"mock"`).
    fn id(&self) -> &'static str;

    /// Static capability bits used by routing and `doctor`.
    fn capabilities(&self) -> Capabilities;

    /// Cheap reachability probe. Must not perform a translation.
    fn health(&self) -> Health;

    /// Run one translation. Errors are normalized into [`BackendError`].
    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError>;
}

/// What a backend can do, independent of the current request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Capabilities {
    /// Context window the backend will actually request (`num_ctx`, etc.).
    pub context_len: u32,
    /// Native structured-output support (schema / tool-use / json_object).
    pub structured_output: bool,
    /// Provider can stream tokens (Ollama NDJSON). Still synchronous (ADR-006).
    pub streaming: bool,
    /// Local vs cloud cost class, used by routing notices.
    pub cost_tier: CostTier,
}

/// Cost class of a backend.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CostTier {
    /// Loopback / local process (Ollama, LM Studio, mock).
    Local,
    /// Remote HTTP API.
    Cloud,
}

/// Result of [`Backend::health`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Health {
    /// Process/endpoint is reachable.
    pub reachable: bool,
    /// Configured model is present (or N/A for key-only cloud).
    pub model_present: bool,
    /// Required credential is available (always true for local/mock).
    pub auth_configured: bool,
    /// Optional one-line hint (`ollama pull qwen3:14b`).
    pub message: Option<String>,
}

/// Normalized provider failure. The router keys off [`ErrorKind`], never
/// provider-specific strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendError {
    /// Machine-readable class.
    pub kind: ErrorKind,
    /// Backend id that failed (`"ollama"`, …).
    pub backend: &'static str,
    /// Whether a retry or escalation is reasonable.
    pub retryable: bool,
    /// Human-readable detail for stderr / `--verbose`.
    pub detail: String,
}

impl std::fmt::Display for BackendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} [{}] retryable={}: {}",
            self.kind, self.backend, self.retryable, self.detail
        )
    }
}

impl std::error::Error for BackendError {}

/// Classes of backend failure the router understands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// Nothing listening / DNS / connection refused.
    Unreachable,
    /// Deadline exceeded.
    Timeout,
    /// Missing or rejected credential.
    Auth,
    /// Endpoint up but the configured model is absent.
    ModelMissing,
    /// Response was not valid JSON against the output contract.
    BadOutput,
    /// Provider rate-limited the caller.
    RateLimit,
    /// 5xx / internal provider error.
    Server,
}

impl std::fmt::Display for ErrorKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Unreachable => "unreachable",
            Self::Timeout => "timeout",
            Self::Auth => "auth",
            Self::ModelMissing => "model_missing",
            Self::BadOutput => "bad_output",
            Self::RateLimit => "rate_limit",
            Self::Server => "server",
        };
        f.write_str(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::{Candidate, OutputSchema, Usage};

    struct Dummy;

    impl Backend for Dummy {
        fn id(&self) -> &'static str {
            "mock"
        }

        fn capabilities(&self) -> Capabilities {
            Capabilities {
                context_len: 4096,
                structured_output: true,
                streaming: false,
                cost_tier: CostTier::Local,
            }
        }

        fn health(&self) -> Health {
            Health {
                reachable: true,
                model_present: true,
                auth_configured: true,
                message: None,
            }
        }

        fn translate(&self, _req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
            Ok(TranslateResponse {
                candidates: vec![Candidate {
                    command: "true".into(),
                    explanation: "fixture".into(),
                    confidence: 1.0,
                }],
                raw: "{}".into(),
                usage: Usage::default(),
                backend_id: self.id().into(),
                model: "dummy".into(),
                latency_ms: 0,
                confidence: Some(1.0),
            })
        }
    }

    #[test]
    fn dummy_backend_satisfies_trait() {
        let b = Dummy;
        assert_eq!(b.id(), "mock");
        assert!(b.health().reachable);
        let req = TranslateRequest {
            system: String::new(),
            user: "noop".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 16,
            temperature: 0.1,
            timeout_ms: 1_000,
        };
        let resp = b.translate(&req).expect("dummy translate");
        assert_eq!(resp.candidates.len(), 1);
    }

    #[test]
    fn error_kind_retryable_contract() {
        let err = BackendError {
            kind: ErrorKind::Timeout,
            backend: "ollama",
            retryable: true,
            detail: "deadline".into(),
        };
        assert!(err.retryable);
        assert_eq!(err.kind, ErrorKind::Timeout);
        let json = serde_json::to_string(&err.kind).unwrap();
        let back: ErrorKind = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ErrorKind::Timeout);
    }

    #[test]
    fn capabilities_and_health_roundtrip() {
        let caps = Capabilities {
            context_len: 4096,
            structured_output: true,
            streaming: false,
            cost_tier: CostTier::Cloud,
        };
        let json = serde_json::to_string(&caps).unwrap();
        let back: Capabilities = serde_json::from_str(&json).unwrap();
        assert_eq!(caps, back);

        let health = Health {
            reachable: false,
            model_present: false,
            auth_configured: true,
            message: Some("ollama pull qwen3:14b".into()),
        };
        let json = serde_json::to_string(&health).unwrap();
        let back: Health = serde_json::from_str(&json).unwrap();
        assert_eq!(health, back);
    }
}
