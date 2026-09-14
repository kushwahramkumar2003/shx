//! Fixture-driven [`Backend`] for tests, `--offline`, and CI.
//!
//! Deterministic: same request always yields the same response. Faults are
//! injected by intent string (`Timeout`, `Unreachable`, …).

use std::collections::BTreeMap;

use shx_core::{Candidate, TranslateRequest, TranslateResponse, Usage};

use crate::backend::{Backend, BackendError, Capabilities, CostTier, ErrorKind, Health};

/// Scripted outcome for one intent.
#[derive(Debug, Clone, PartialEq)]
enum Script {
    /// Canned successful translation.
    Ok {
        command: String,
        explanation: String,
        confidence: f32,
    },
    /// Injected provider failure.
    Fault(ErrorKind),
}

/// In-process backend. No network.
#[derive(Debug, Clone)]
pub struct MockBackend {
    scripts: BTreeMap<String, Script>,
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    /// Default fixtures used by `--offline` and CLI tests.
    pub fn new() -> Self {
        let mut this = Self {
            scripts: BTreeMap::new(),
        };
        this.script_ok(
            "run pg on 7000",
            "docker run --name pg -e POSTGRES_PASSWORD=postgres -p 7000:5432 -d postgres:16",
            "Starts a detached Postgres 16 container named pg, mapping host port 7000 to container 5432.",
            0.95,
        );
        this
    }

    /// Empty mock: unknown intents still return a deterministic fallback.
    pub fn empty() -> Self {
        Self {
            scripts: BTreeMap::new(),
        }
    }

    /// Script a successful translation for `intent`.
    pub fn script_ok(
        &mut self,
        intent: impl Into<String>,
        command: impl Into<String>,
        explanation: impl Into<String>,
        confidence: f32,
    ) -> &mut Self {
        self.scripts.insert(
            intent.into(),
            Script::Ok {
                command: command.into(),
                explanation: explanation.into(),
                confidence,
            },
        );
        self
    }

    /// Script a fault for `intent`.
    pub fn script_fault(&mut self, intent: impl Into<String>, kind: ErrorKind) -> &mut Self {
        self.scripts.insert(intent.into(), Script::Fault(kind));
        self
    }

    /// Whether `kind` is treated as retryable / escalation-worthy.
    pub fn retryable(kind: ErrorKind) -> bool {
        match kind {
            ErrorKind::Timeout
            | ErrorKind::Unreachable
            | ErrorKind::BadOutput
            | ErrorKind::RateLimit
            | ErrorKind::Server => true,
            ErrorKind::Auth | ErrorKind::ModelMissing => false,
        }
    }
}

/// Pull the intent line out of a prompt built by `PromptBuilder`.
pub fn intent_key(req: &TranslateRequest) -> String {
    for line in req.user.lines().rev() {
        if let Some(rest) = line.strip_prefix("INTENT: ") {
            return rest.to_string();
        }
    }
    req.user.trim().to_string()
}

fn error_for(kind: ErrorKind) -> BackendError {
    let detail = match kind {
        ErrorKind::Timeout => "mock timeout",
        ErrorKind::Unreachable => "mock unreachable",
        ErrorKind::Auth => "mock auth failure",
        ErrorKind::ModelMissing => "mock model missing; run: ollama pull fixture",
        ErrorKind::BadOutput => "mock bad output",
        ErrorKind::RateLimit => "mock rate limit",
        ErrorKind::Server => "mock server error",
    };
    BackendError {
        kind,
        backend: "mock",
        retryable: MockBackend::retryable(kind),
        detail: detail.into(),
    }
}

fn ok_response(command: String, explanation: String, confidence: f32) -> TranslateResponse {
    TranslateResponse {
        candidates: vec![Candidate {
            command,
            explanation,
            confidence,
        }],
        raw: String::new(),
        usage: Usage::default(),
        backend_id: "mock".into(),
        model: "fixture".into(),
        latency_ms: 0,
        confidence: Some(confidence),
    }
}

impl Backend for MockBackend {
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
            message: Some("mock backend".into()),
        }
    }

    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
        let key = intent_key(req);
        match self.scripts.get(&key) {
            Some(Script::Ok {
                command,
                explanation,
                confidence,
            }) => Ok(ok_response(
                command.clone(),
                explanation.clone(),
                *confidence,
            )),
            Some(Script::Fault(kind)) => Err(error_for(*kind)),
            None => Ok(ok_response(
                format!("true # mock: {key}"),
                "deterministic fallback for an unscripted intent".into(),
                0.5,
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::{Intent, IntentFlags, NoopRedactor, OutputSchema, PromptBuilder};

    fn req_for(intent: &str) -> TranslateRequest {
        TranslateRequest {
            system: String::new(),
            user: format!("INTENT: {intent}"),
            schema: OutputSchema::translate_v1(),
            max_tokens: 64,
            temperature: 0.1,
            timeout_ms: 1_000,
        }
    }

    #[test]
    fn each_injected_fault() {
        let cases = [
            (ErrorKind::Timeout, true),
            (ErrorKind::Unreachable, true),
            (ErrorKind::Auth, false),
            (ErrorKind::ModelMissing, false),
            (ErrorKind::BadOutput, true),
            (ErrorKind::RateLimit, true),
        ];
        for (kind, retryable) in cases {
            let mut mock = MockBackend::empty();
            mock.script_fault("boom", kind);
            let err = mock.translate(&req_for("boom")).expect_err("fault");
            assert_eq!(err.kind, kind, "{kind}");
            assert_eq!(err.retryable, retryable, "{kind}");
            assert_eq!(err.backend, "mock");
        }
    }

    #[test]
    fn deterministic_same_input_same_output() {
        let mock = MockBackend::new();
        let a = mock.translate(&req_for("run pg on 7000")).unwrap();
        let b = mock.translate(&req_for("run pg on 7000")).unwrap();
        assert_eq!(a, b);
        assert!(a.candidates[0].command.contains("postgres:16"));
        assert_eq!(a.confidence, Some(0.95));
    }

    #[test]
    fn scripted_confidence() {
        let mut mock = MockBackend::empty();
        mock.script_ok("x", "true", "noop", 0.42);
        let resp = mock.translate(&req_for("x")).unwrap();
        assert_eq!(resp.confidence, Some(0.42));
        assert!((resp.candidates[0].confidence - 0.42).abs() < f32::EPSILON);
    }

    #[test]
    fn extracts_intent_from_prompt_builder_user() {
        let intent = Intent {
            text: "run pg on 7000".into(),
            force_backend: None,
            count: 1,
            flags: IntentFlags::default(),
        };
        let ctx = shx_core::ContextBundle {
            env: shx_core::EnvInfo {
                os: "macos".into(),
                shell: "zsh".into(),
                cwd: "/tmp".into(),
                git_root: None,
                in_container: false,
            },
            profile: shx_core::Profile {
                name: "default".into(),
                ports: vec![],
                prefer_docker: true,
                notes: String::new(),
            },
            history: vec![],
            vocabulary: vec![],
            snippets: vec![],
            shell: vec![],
        };
        let req = PromptBuilder.build(&intent, &ctx, &NoopRedactor);
        let mock = MockBackend::new();
        let resp = mock.translate(&req).unwrap();
        assert!(resp.candidates[0].command.contains("7000:5432"));
    }
}
