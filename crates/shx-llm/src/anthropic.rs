//! Anthropic backend: forced `tool_use` structured output (docs/06-BACKENDS.md §4).
//!
//! The API key is read from the env var *named* in config (`api_key_env`), with
//! `SHX_CLOUD_API_KEY` taking precedence. The key is never stored in the file.

use std::env;
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use shx_core::{TranslateRequest, TranslateResponse, Usage, parse_response};

use crate::backend::{Backend, BackendError, Capabilities, CostTier, ErrorKind, Health};
use crate::http::{Transport, TransportError, UreqTransport};

/// Anthropic Messages API version header.
pub const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Tool name sent as the single forced tool (matches [`shx_core::OutputSchema::name`]).
pub const TOOL_NAME: &str = "shx_translate";

/// Connection settings. Mirrors `[backend.cloud]` without depending on `shx-config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnthropicSettings {
    /// Model id (`claude-sonnet-4-5`).
    pub model: String,
    /// Name of the env var that holds the key — never the key itself.
    pub api_key_env: String,
    /// Per-attempt HTTP timeout.
    pub timeout_ms: u64,
    /// `https://api.anthropic.com`
    pub base_url: String,
}

impl Default for AnthropicSettings {
    fn default() -> Self {
        Self {
            model: "claude-sonnet-4-5".into(),
            api_key_env: "ANTHROPIC_API_KEY".into(),
            timeout_ms: 20_000,
            base_url: "https://api.anthropic.com".into(),
        }
    }
}

/// Where the API key is resolved from.
#[derive(Debug, Clone)]
enum KeySource {
    /// Production: `SHX_CLOUD_API_KEY` then `settings.api_key_env`.
    Env,
    /// Tests: injected so a developer's real key cannot leak into fixtures.
    Fixture(Option<String>),
}

/// Cloud Anthropic provider.
pub struct AnthropicBackend {
    settings: AnthropicSettings,
    transport: Box<dyn Transport>,
    keys: KeySource,
}

impl AnthropicBackend {
    /// Production backend using [`UreqTransport`] and process env.
    pub fn new(settings: AnthropicSettings) -> Self {
        Self {
            settings,
            transport: Box::new(UreqTransport),
            keys: KeySource::Env,
        }
    }

    /// Inject a transport and an optional fixture key (unit tests).
    pub fn with_transport(
        settings: AnthropicSettings,
        transport: impl Transport + 'static,
        api_key: Option<String>,
    ) -> Self {
        Self {
            settings,
            transport: Box::new(transport),
            keys: KeySource::Fixture(api_key),
        }
    }

    /// JSON body sent to `/v1/messages`. Single forced tool; `input_schema` is the contract.
    pub fn messages_request_body(settings: &AnthropicSettings, req: &TranslateRequest) -> Value {
        json!({
            "model": settings.model,
            "max_tokens": req.max_tokens,
            "temperature": req.temperature,
            "system": req.system,
            "messages": [
                { "role": "user", "content": req.user }
            ],
            "tools": [
                {
                    "name": TOOL_NAME,
                    "description": "Return the shell-command translation as structured JSON.",
                    "input_schema": req.schema.json
                }
            ],
            "tool_choice": { "type": "tool", "name": TOOL_NAME }
        })
    }

    /// Resolve the API key: `SHX_CLOUD_API_KEY` then the named provider env var.
    pub fn resolve_api_key_from(
        api_key_env: &str,
        get: impl Fn(&str) -> Option<String>,
    ) -> Option<String> {
        for name in ["SHX_CLOUD_API_KEY", api_key_env] {
            if let Some(v) = get(name)
                && !v.is_empty()
            {
                return Some(v);
            }
        }
        None
    }

    fn api_key(&self) -> Option<String> {
        match &self.keys {
            KeySource::Env => {
                Self::resolve_api_key_from(&self.settings.api_key_env, |n| env::var(n).ok())
            }
            KeySource::Fixture(k) => k.clone(),
        }
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.settings.timeout_ms.max(1))
    }

    fn url(&self, path: &str) -> String {
        let base = self.settings.base_url.trim_end_matches('/');
        format!("{base}{path}")
    }

    fn map_transport(&self, err: TransportError) -> BackendError {
        crate::error::from_transport("anthropic", err, None)
    }

    fn missing_key(&self) -> BackendError {
        BackendError {
            kind: ErrorKind::Auth,
            backend: "anthropic",
            retryable: false,
            detail: format!(
                "missing API key; set {} or SHX_CLOUD_API_KEY (the name from backend.cloud.api_key_env)",
                self.settings.api_key_env
            ),
        }
    }
}

impl Backend for AnthropicBackend {
    fn id(&self) -> &'static str {
        "anthropic"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            context_len: 200_000,
            structured_output: true,
            streaming: false,
            cost_tier: CostTier::Cloud,
        }
    }

    fn health(&self) -> Health {
        let auth = self.api_key().is_some();
        Health {
            reachable: true,
            model_present: true,
            auth_configured: auth,
            message: if auth {
                None
            } else {
                Some(format!(
                    "set {} or SHX_CLOUD_API_KEY",
                    self.settings.api_key_env
                ))
            },
        }
    }

    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
        let key = self.api_key().ok_or_else(|| self.missing_key())?;
        let body = Self::messages_request_body(&self.settings, req);
        let headers = [
            ("x-api-key", key.as_str()),
            ("anthropic-version", ANTHROPIC_VERSION),
        ];
        let started = Instant::now();
        let resp = self
            .transport
            .post_json_with_headers(&self.url("/v1/messages"), &body, self.timeout(), &headers)
            .map_err(|e| self.map_transport(e))?;
        let latency_ms = started.elapsed().as_millis() as u64;
        parse_messages_response(&resp.body, &self.settings.model, latency_ms)
    }
}

fn parse_messages_response(
    body: &str,
    fallback_model: &str,
    latency_ms: u64,
) -> Result<TranslateResponse, BackendError> {
    let envelope: Value = serde_json::from_str(body).map_err(|e| {
        crate::error::from_bad_output("anthropic", format!("anthropic envelope: {e}"))
    })?;
    let input = tool_input(&envelope).ok_or_else(|| {
        crate::error::from_bad_output(
            "anthropic",
            "anthropic response missing tool_use input for shx_translate",
        )
    })?;
    let raw = input.to_string();
    let parsed = parse_response(&raw)
        .map_err(|e| crate::error::from_bad_output("anthropic", e.to_string()))?;
    let confidence = parsed.candidates.first().map(|c| c.confidence);
    let usage = Usage {
        prompt_tokens: envelope
            .pointer("/usage/input_tokens")
            .and_then(|n| n.as_u64())
            .map(|n| n as u32),
        completion_tokens: envelope
            .pointer("/usage/output_tokens")
            .and_then(|n| n.as_u64())
            .map(|n| n as u32),
    };
    let model = envelope
        .get("model")
        .and_then(|m| m.as_str())
        .unwrap_or(fallback_model)
        .to_string();
    Ok(TranslateResponse {
        candidates: parsed.candidates,
        raw,
        usage,
        backend_id: "anthropic".into(),
        model,
        latency_ms,
        confidence,
    })
}

fn tool_input(envelope: &Value) -> Option<&Value> {
    let blocks = envelope.get("content")?.as_array()?;
    blocks.iter().find_map(|b| {
        let ty = b.get("type")?.as_str()?;
        let name = b.get("name")?.as_str()?;
        if ty == "tool_use" && name == TOOL_NAME {
            b.get("input")
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use shx_core::{Candidate, OutputSchema};

    use crate::http::HttpResponse;

    type HeaderLog = std::sync::Arc<Mutex<Option<Vec<(String, String)>>>>;

    struct Scripted {
        post: Mutex<Result<HttpResponse, TransportError>>,
        last_post: std::sync::Arc<Mutex<Option<Value>>>,
        last_headers: HeaderLog,
        last_url: std::sync::Arc<Mutex<Option<String>>>,
    }

    impl Transport for Scripted {
        fn get(&self, _url: &str, _timeout: Duration) -> Result<HttpResponse, TransportError> {
            Err(TransportError::Unreachable("unused".into()))
        }

        fn post_json_with_headers(
            &self,
            url: &str,
            body: &Value,
            _timeout: Duration,
            headers: &[(&str, &str)],
        ) -> Result<HttpResponse, TransportError> {
            *self.last_url.lock().expect("url") = Some(url.to_string());
            *self.last_post.lock().expect("body") = Some(body.clone());
            *self.last_headers.lock().expect("headers") = Some(
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            );
            self.post.lock().expect("post").clone()
        }
    }

    fn unused_scripted() -> Scripted {
        Scripted {
            post: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            last_post: std::sync::Arc::new(Mutex::new(None)),
            last_headers: std::sync::Arc::new(Mutex::new(None)),
            last_url: std::sync::Arc::new(Mutex::new(None)),
        }
    }

    fn settings() -> AnthropicSettings {
        AnthropicSettings::default()
    }

    fn sample_req() -> TranslateRequest {
        TranslateRequest {
            system: "sys".into(),
            user: "INTENT: run pg on 7000".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 256,
            temperature: 0.1,
            timeout_ms: 20_000,
        }
    }

    fn tool_envelope(input: Value) -> Value {
        json!({
            "id": "msg_test",
            "type": "message",
            "role": "assistant",
            "model": "claude-sonnet-4-5",
            "content": [
                {
                    "type": "tool_use",
                    "id": "toolu_test",
                    "name": TOOL_NAME,
                    "input": input
                }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 12, "output_tokens": 4 }
        })
    }

    #[test]
    fn request_body_has_single_tool_input_schema() {
        let req = sample_req();
        let body = AnthropicBackend::messages_request_body(&settings(), &req);
        let tools = body["tools"].as_array().expect("tools");
        assert_eq!(tools.len(), 1, "exactly one tool");
        assert_eq!(body["tools"][0]["name"], TOOL_NAME);
        assert_eq!(body["tool_choice"]["type"], "tool");
        assert_eq!(body["tool_choice"]["name"], TOOL_NAME);
        assert_eq!(body["tools"][0]["input_schema"]["type"], "object");
        assert_eq!(body["tools"][0]["input_schema"]["required"][0], "commands");
        assert_eq!(body["tools"][0]["input_schema"], req.schema.json);
        assert_eq!(body["model"], "claude-sonnet-4-5");
        assert_eq!(body["max_tokens"], 256);
        assert_eq!(body["system"], "sys");
        assert_eq!(body["messages"][0]["role"], "user");
        assert_eq!(body["messages"][0]["content"], "INTENT: run pg on 7000");
        assert!(
            body.get("api_key").is_none() && body.get("x-api-key").is_none(),
            "key must not appear in the JSON body"
        );
    }

    #[test]
    fn key_from_named_env_never_a_file_value() {
        let from_named = AnthropicBackend::resolve_api_key_from("ANTHROPIC_API_KEY", |n| match n {
            "ANTHROPIC_API_KEY" => Some("sk-ant-from-env".into()),
            _ => None,
        });
        assert_eq!(from_named.as_deref(), Some("sk-ant-from-env"));

        let cloud_wins = AnthropicBackend::resolve_api_key_from("ANTHROPIC_API_KEY", |n| match n {
            "SHX_CLOUD_API_KEY" => Some("sk-cloud".into()),
            "ANTHROPIC_API_KEY" => Some("sk-provider".into()),
            _ => None,
        });
        assert_eq!(cloud_wins.as_deref(), Some("sk-cloud"));

        let empty =
            AnthropicBackend::resolve_api_key_from("ANTHROPIC_API_KEY", |_| Some(String::new()));
        assert!(empty.is_none());

        let settings = AnthropicSettings::default();
        let debug = format!("{settings:?}");
        assert!(debug.contains("ANTHROPIC_API_KEY"));
        assert!(
            !debug.contains("sk-ant"),
            "settings debug must not hold a key"
        );
    }

    #[test]
    fn missing_key_is_auth_and_skips_http() {
        let t = unused_scripted();
        let last_url = std::sync::Arc::clone(&t.last_url);
        let b = AnthropicBackend::with_transport(settings(), t, None);
        let h = b.health();
        assert!(!h.auth_configured);
        let err = b.translate(&sample_req()).expect_err("auth");
        assert_eq!(err.kind, ErrorKind::Auth);
        assert!(!err.retryable);
        assert!(err.detail.contains("ANTHROPIC_API_KEY"), "{}", err.detail);
        assert!(
            last_url.lock().expect("url").is_none(),
            "missing key must not hit the network"
        );
    }

    #[test]
    fn http_401_is_auth() {
        let t = Scripted {
            post: Mutex::new(Err(TransportError::Status {
                code: 401,
                body: "invalid x-api-key".into(),
            })),
            last_post: std::sync::Arc::new(Mutex::new(None)),
            last_headers: std::sync::Arc::new(Mutex::new(None)),
            last_url: std::sync::Arc::new(Mutex::new(None)),
        };
        let b = AnthropicBackend::with_transport(settings(), t, Some("sk-ant-fixture".into()));
        let err = b.translate(&sample_req()).expect_err("401");
        assert_eq!(err.kind, ErrorKind::Auth);
        assert!(!err.retryable);
        assert_eq!(err.backend, "anthropic");
    }

    #[test]
    fn tool_use_response_maps_to_translate_response() {
        let input = json!({
            "commands": [
                {
                    "command": "docker run pg",
                    "explanation": "start postgres",
                    "confidence": 0.91
                }
            ],
            "risk_notes": [],
            "assumptions": []
        });
        let t = Scripted {
            post: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: tool_envelope(input).to_string(),
            })),
            last_post: std::sync::Arc::new(Mutex::new(None)),
            last_headers: std::sync::Arc::new(Mutex::new(None)),
            last_url: std::sync::Arc::new(Mutex::new(None)),
        };
        let b = AnthropicBackend::with_transport(settings(), t, Some("sk-ant-fixture".into()));
        let resp = b.translate(&sample_req()).expect("ok");
        assert_eq!(resp.backend_id, "anthropic");
        assert_eq!(resp.model, "claude-sonnet-4-5");
        assert_eq!(
            resp.candidates,
            vec![Candidate {
                command: "docker run pg".into(),
                explanation: "start postgres".into(),
                confidence: 0.91,
            }]
        );
        assert_eq!(resp.confidence, Some(0.91));
        assert_eq!(resp.usage.prompt_tokens, Some(12));
        assert_eq!(resp.usage.completion_tokens, Some(4));
    }

    #[test]
    fn missing_tool_use_is_bad_output() {
        let t = Scripted {
            post: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: json!({
                    "content": [{ "type": "text", "text": "hello" }],
                    "model": "claude-sonnet-4-5"
                })
                .to_string(),
            })),
            last_post: std::sync::Arc::new(Mutex::new(None)),
            last_headers: std::sync::Arc::new(Mutex::new(None)),
            last_url: std::sync::Arc::new(Mutex::new(None)),
        };
        let b = AnthropicBackend::with_transport(settings(), t, Some("sk-ant-fixture".into()));
        let err = b.translate(&sample_req()).expect_err("bad output");
        assert_eq!(err.kind, ErrorKind::BadOutput);
    }

    #[test]
    fn sends_version_key_header_and_messages_url() {
        let input = json!({
            "commands": [{ "command": "true", "explanation": "noop", "confidence": 0.5 }],
            "risk_notes": [],
            "assumptions": []
        });
        let t = Scripted {
            post: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: tool_envelope(input).to_string(),
            })),
            last_post: std::sync::Arc::new(Mutex::new(None)),
            last_headers: std::sync::Arc::new(Mutex::new(None)),
            last_url: std::sync::Arc::new(Mutex::new(None)),
        };
        let headers = std::sync::Arc::clone(&t.last_headers);
        let url = std::sync::Arc::clone(&t.last_url);
        let body = std::sync::Arc::clone(&t.last_post);
        let b = AnthropicBackend::with_transport(settings(), t, Some("sk-ant-fixture".into()));
        b.translate(&sample_req()).expect("ok");
        let url = url.lock().expect("url").clone().expect("posted");
        assert!(url.ends_with("/v1/messages"), "{url}");
        let headers = headers.lock().expect("h").clone().expect("headers");
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "x-api-key" && v == "sk-ant-fixture"),
            "{headers:?}"
        );
        assert!(
            headers
                .iter()
                .any(|(k, v)| k == "anthropic-version" && v == ANTHROPIC_VERSION),
            "{headers:?}"
        );
        let posted = body.lock().expect("body").clone().expect("json");
        assert_eq!(posted["tools"].as_array().map(|a| a.len()), Some(1));
        assert!(posted.get("x-api-key").is_none());
    }
}
