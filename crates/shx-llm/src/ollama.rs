//! Ollama backend: `POST /api/chat` + `GET /api/tags` (docs/06-BACKENDS.md §4).

use std::time::{Duration, Instant};

use serde_json::{Value, json};
use shx_core::{TranslateRequest, TranslateResponse, Usage, parse_response};

use crate::backend::{Backend, BackendError, Capabilities, CostTier, Health};
use crate::http::{HttpResponse, Transport, TransportError, UreqTransport};

/// Connection settings. Mirrors `[backend.local]` without depending on `shx-config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OllamaSettings {
    /// `http://127.0.0.1:11434`
    pub base_url: String,
    /// Model id (`qwen3:14b`).
    pub model: String,
    /// Ollama keep_alive duration (`30m`).
    pub keep_alive: String,
    /// Requested `num_ctx`.
    pub num_ctx: u32,
    /// Per-attempt HTTP timeout.
    pub timeout_ms: u64,
}

impl Default for OllamaSettings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434".into(),
            model: "qwen3:14b".into(),
            keep_alive: "30m".into(),
            num_ctx: 4096,
            timeout_ms: 8_000,
        }
    }
}

/// Local Ollama provider.
pub struct OllamaBackend {
    settings: OllamaSettings,
    transport: Box<dyn Transport>,
}

impl OllamaBackend {
    /// Production backend using [`UreqTransport`].
    pub fn new(settings: OllamaSettings) -> Self {
        Self {
            settings,
            transport: Box::new(UreqTransport),
        }
    }

    /// Inject a transport (unit tests).
    pub fn with_transport(settings: OllamaSettings, transport: impl Transport + 'static) -> Self {
        Self {
            settings,
            transport: Box::new(transport),
        }
    }

    /// JSON body sent to `/api/chat`. Unit-tested for keep_alive / num_ctx / schema.
    pub fn chat_request_body(settings: &OllamaSettings, req: &TranslateRequest) -> Value {
        json!({
            "model": settings.model,
            "messages": [
                { "role": "system", "content": req.system },
                { "role": "user", "content": req.user }
            ],
            "stream": false,
            "keep_alive": settings.keep_alive,
            "format": req.schema.json,
            "options": {
                "temperature": req.temperature,
                "num_ctx": settings.num_ctx,
                "num_predict": req.max_tokens
            }
        })
    }

    fn timeout(&self) -> Duration {
        Duration::from_millis(self.settings.timeout_ms.max(1))
    }

    fn url(&self, path: &str) -> String {
        let base = self.settings.base_url.trim_end_matches('/');
        format!("{base}{path}")
    }

    fn pull_hint(&self) -> String {
        format!("run: ollama pull {}", self.settings.model)
    }

    fn map_transport(&self, err: TransportError) -> BackendError {
        crate::error::from_transport("ollama", err, Some(&self.pull_hint()))
    }

    fn model_listed(&self, tags_body: &str) -> bool {
        let Ok(v) = serde_json::from_str::<Value>(tags_body) else {
            return false;
        };
        let Some(models) = v.get("models").and_then(|m| m.as_array()) else {
            return false;
        };
        let want = self.settings.model.as_str();
        models.iter().any(|m| {
            let name = m.get("name").and_then(|n| n.as_str()).unwrap_or("");
            name == want
                || name == format!("{want}:latest")
                || name.starts_with(&format!("{want}:"))
        })
    }
}

impl Backend for OllamaBackend {
    fn id(&self) -> &'static str {
        "ollama"
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            context_len: self.settings.num_ctx,
            structured_output: true,
            streaming: false,
            cost_tier: CostTier::Local,
        }
    }

    fn health(&self) -> Health {
        match self.transport.get(&self.url("/api/tags"), self.timeout()) {
            Ok(HttpResponse { body, .. }) => {
                let present = self.model_listed(&body);
                Health {
                    reachable: true,
                    model_present: present,
                    auth_configured: true,
                    message: if present {
                        None
                    } else {
                        Some(self.pull_hint())
                    },
                }
            }
            Err(_) => Health {
                reachable: false,
                model_present: false,
                auth_configured: true,
                message: Some(format!(
                    "ollama unreachable at {}; is it running?",
                    self.settings.base_url
                )),
            },
        }
    }

    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
        let body = Self::chat_request_body(&self.settings, req);
        let started = Instant::now();
        let resp = self
            .transport
            .post_json(&self.url("/api/chat"), &body, self.timeout())
            .map_err(|e| self.map_transport(e))?;
        let latency_ms = started.elapsed().as_millis() as u64;

        let envelope: Value = serde_json::from_str(&resp.body).map_err(|e| {
            crate::error::from_bad_output("ollama", format!("ollama envelope: {e}"))
        })?;
        let content = envelope
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(|c| c.as_str())
            .ok_or_else(|| {
                crate::error::from_bad_output("ollama", "ollama response missing message.content")
            })?;

        let parsed = parse_response(content)
            .map_err(|e| crate::error::from_bad_output("ollama", e.to_string()))?;
        let confidence = parsed.candidates.first().map(|c| c.confidence);
        let usage = Usage {
            prompt_tokens: envelope
                .get("prompt_eval_count")
                .and_then(|n| n.as_u64())
                .map(|n| n as u32),
            completion_tokens: envelope
                .get("eval_count")
                .and_then(|n| n.as_u64())
                .map(|n| n as u32),
        };
        let model = envelope
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&self.settings.model)
            .to_string();

        Ok(TranslateResponse {
            candidates: parsed.candidates,
            raw: content.to_string(),
            usage,
            backend_id: "ollama".into(),
            model,
            latency_ms,
            confidence,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    use shx_core::{Intent, IntentFlags, NoopRedactor, OutputSchema, PromptBuilder};

    use crate::backend::ErrorKind;

    struct Scripted {
        get: Mutex<Result<HttpResponse, TransportError>>,
        post: Mutex<Result<HttpResponse, TransportError>>,
        last_post: Mutex<Option<Value>>,
    }

    impl Transport for Scripted {
        fn get(&self, _url: &str, _timeout: Duration) -> Result<HttpResponse, TransportError> {
            self.get.lock().expect("get").clone()
        }

        fn post_json(
            &self,
            _url: &str,
            body: &Value,
            _timeout: Duration,
        ) -> Result<HttpResponse, TransportError> {
            *self.last_post.lock().expect("last") = Some(body.clone());
            self.post.lock().expect("post").clone()
        }
    }

    fn settings() -> OllamaSettings {
        OllamaSettings::default()
    }

    fn sample_req() -> TranslateRequest {
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
                ports: vec![7000],
                prefer_docker: true,
                notes: String::new(),
            },
            history: vec![],
            vocabulary: vec![],
            snippets: vec![],
            shell: vec![],
        };
        PromptBuilder.build(&intent, &ctx, &NoopRedactor)
    }

    #[test]
    fn chat_body_has_keep_alive_num_ctx_schema() {
        let req = TranslateRequest {
            system: "sys".into(),
            user: "INTENT: run pg on 7000".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 256,
            temperature: 0.1,
            timeout_ms: 8_000,
        };
        let body = OllamaBackend::chat_request_body(&settings(), &req);
        assert_eq!(body["keep_alive"], "30m");
        assert_eq!(body["options"]["num_ctx"], 4096);
        assert_eq!(body["stream"], false);
        assert_eq!(body["model"], "qwen3:14b");
        assert!(
            body["format"].is_object(),
            "format must be the JSON schema object"
        );
        assert_eq!(body["format"]["type"], "object");
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["role"], "user");
    }

    #[test]
    fn health_reachable_and_present() {
        let t = Scripted {
            get: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: r#"{"models":[{"name":"qwen3:14b"}]}"#.into(),
            })),
            post: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            last_post: Mutex::new(None),
        };
        let b = OllamaBackend::with_transport(settings(), t);
        let h = b.health();
        assert!(h.reachable);
        assert!(h.model_present);
        assert!(h.message.is_none());
    }

    #[test]
    fn health_reachable_missing_has_pull_hint() {
        let t = Scripted {
            get: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: r#"{"models":[{"name":"llama3:latest"}]}"#.into(),
            })),
            post: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            last_post: Mutex::new(None),
        };
        let b = OllamaBackend::with_transport(settings(), t);
        let h = b.health();
        assert!(h.reachable);
        assert!(!h.model_present);
        let msg = h.message.expect("pull hint");
        assert!(msg.contains("ollama pull qwen3:14b"), "{msg}");
    }

    #[test]
    fn health_unreachable() {
        let t = Scripted {
            get: Mutex::new(Err(TransportError::Unreachable(
                "connection refused".into(),
            ))),
            post: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            last_post: Mutex::new(None),
        };
        let b = OllamaBackend::with_transport(settings(), t);
        let h = b.health();
        assert!(!h.reachable);
        assert!(!h.model_present);
        assert!(h.message.unwrap().contains("unreachable"));
    }

    #[test]
    fn fixture_chat_response_parses() {
        let content = r#"{"commands":[{"command":"true","explanation":"noop","confidence":0.9}],"risk_notes":[],"assumptions":[]}"#;
        let envelope = json!({
            "model": "qwen3:14b",
            "message": { "role": "assistant", "content": content },
            "done": true,
            "prompt_eval_count": 12,
            "eval_count": 4
        });
        let t = Scripted {
            get: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            post: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: envelope.to_string(),
            })),
            last_post: Mutex::new(None),
        };
        let b = OllamaBackend::with_transport(settings(), t);
        let resp = b.translate(&sample_req()).expect("parse fixture");
        assert_eq!(resp.candidates[0].command, "true");
        assert_eq!(resp.usage.prompt_tokens, Some(12));
        assert_eq!(resp.backend_id, "ollama");
    }

    #[test]
    fn model_missing_status_maps_error() {
        let t = Scripted {
            get: Mutex::new(Err(TransportError::Unreachable("unused".into()))),
            post: Mutex::new(Err(TransportError::Status {
                code: 404,
                body: r#"{"error":"model 'qwen3:14b' not found, try pulling it first"}"#.into(),
            })),
            last_post: Mutex::new(None),
        };
        let b = OllamaBackend::with_transport(settings(), t);
        let err = b.translate(&sample_req()).expect_err("missing");
        assert_eq!(err.kind, ErrorKind::ModelMissing);
        assert!(!err.retryable);
        assert!(err.detail.contains("ollama pull qwen3:14b"));
    }

    /// Live smoke: needs Ollama on 127.0.0.1:11434 with `qwen3:14b`.
    ///
    /// ```sh
    /// SHX_LIVE=1 cargo test -p shx-llm --test lib -- --ignored live_smoke
    /// ```
    #[test]
    #[ignore = "needs a running Ollama with qwen3:14b; SHX_LIVE=1 cargo test -p shx-llm -- --ignored"]
    fn live_smoke() {
        let b = OllamaBackend::new(OllamaSettings::default());
        let h = b.health();
        assert!(h.reachable, "start ollama");
        if !h.model_present {
            panic!("{}", h.message.unwrap_or_else(|| b.pull_hint()));
        }
        let resp = b.translate(&sample_req()).expect("live translate");
        assert!(!resp.candidates.is_empty());
        assert!(!resp.candidates[0].command.is_empty());
    }
}
