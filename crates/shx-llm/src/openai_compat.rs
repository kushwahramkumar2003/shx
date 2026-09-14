//! OpenAI-compatible chat completions + per-`base_url` structured-output probe
//! (docs/06-BACKENDS.md §4).
//!
//! Tries `response_format: json_schema` first. If the endpoint rejects it,
//! downgrades to `json_object` + schema-in-prompt and caches the result in
//! [`MetaCache`] keyed by `base_url`. A missing API key is allowed (LM Studio).

use std::collections::HashMap;
use std::env;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::{Value, json};
use shx_core::{TranslateRequest, TranslateResponse, Usage, parse_response};

use crate::backend::{Backend, BackendError, Capabilities, CostTier, Health};
use crate::http::{Transport, TransportError, UreqTransport};

/// Structured-output mode discovered by the capability probe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredFormat {
    /// `response_format.type = json_schema` with the output contract.
    JsonSchema,
    /// `response_format.type = json_object` plus schema text in the system prompt.
    JsonObject,
}

/// Process-local meta map: `openai_compat.format:{base_url}` → format.
///
/// Shared across backends in tests; one-shot CLI processes get a fresh map.
#[derive(Debug, Default)]
pub struct MetaCache {
    inner: Mutex<HashMap<String, StructuredFormat>>,
}

impl MetaCache {
    /// Empty cache.
    pub fn new() -> Self {
        Self::default()
    }

    fn key(base_url: &str) -> String {
        format!("openai_compat.format:{}", base_url.trim_end_matches('/'))
    }

    /// Cached format for this endpoint, if probed.
    pub fn get(&self, base_url: &str) -> Option<StructuredFormat> {
        let map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.get(&Self::key(base_url)).copied()
    }

    /// Remember the probe result for this endpoint.
    pub fn set(&self, base_url: &str, format: StructuredFormat) {
        let mut map = self.inner.lock().unwrap_or_else(|p| p.into_inner());
        map.insert(Self::key(base_url), format);
    }
}

/// Connection settings. Mirrors `[backend.cloud]` without depending on `shx-config`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenAiCompatSettings {
    /// Chat completions root, including `/v1` when the server uses it.
    pub base_url: String,
    /// Model id.
    pub model: String,
    /// Env var *name* for the key. Empty = never send `Authorization` (LM Studio).
    pub api_key_env: String,
    /// Per-attempt HTTP timeout.
    pub timeout_ms: u64,
}

impl Default for OpenAiCompatSettings {
    fn default() -> Self {
        Self {
            base_url: "https://api.openai.com/v1".into(),
            model: "gpt-4o-mini".into(),
            api_key_env: "OPENAI_API_KEY".into(),
            timeout_ms: 20_000,
        }
    }
}

enum KeySource {
    Env,
    Fixture(Option<String>),
}

/// OpenAI-compatible provider (OpenAI, OpenRouter, LM Studio, vLLM, …).
pub struct OpenAiCompatBackend {
    settings: OpenAiCompatSettings,
    transport: Box<dyn Transport>,
    keys: KeySource,
    meta: Arc<MetaCache>,
}

impl OpenAiCompatBackend {
    /// Production backend using [`UreqTransport`], process env, and a fresh meta cache.
    pub fn new(settings: OpenAiCompatSettings) -> Self {
        Self {
            settings,
            transport: Box::new(UreqTransport),
            keys: KeySource::Env,
            meta: Arc::new(MetaCache::new()),
        }
    }

    /// Inject transport, optional fixture key, and a shared [`MetaCache`] (unit tests).
    pub fn with_transport(
        settings: OpenAiCompatSettings,
        transport: impl Transport + 'static,
        api_key: Option<String>,
        meta: Arc<MetaCache>,
    ) -> Self {
        Self {
            settings,
            transport: Box::new(transport),
            keys: KeySource::Fixture(api_key),
            meta,
        }
    }

    /// Shared meta cache (probe results).
    pub fn meta(&self) -> &MetaCache {
        &self.meta
    }

    /// Chat-completions JSON body for `format`.
    pub fn chat_request_body(
        settings: &OpenAiCompatSettings,
        req: &TranslateRequest,
        format: StructuredFormat,
    ) -> Value {
        let (system, response_format) = match format {
            StructuredFormat::JsonSchema => (
                req.system.clone(),
                json!({
                    "type": "json_schema",
                    "json_schema": {
                        "name": req.schema.name,
                        "schema": req.schema.json,
                        "strict": false
                    }
                }),
            ),
            StructuredFormat::JsonObject => (
                format!(
                    "{}\n\nRespond with a JSON object matching this schema:\n{}",
                    req.system, req.schema.json
                ),
                json!({ "type": "json_object" }),
            ),
        };
        json!({
            "model": settings.model,
            "messages": [
                { "role": "system", "content": system },
                { "role": "user", "content": req.user }
            ],
            "temperature": req.temperature,
            "max_tokens": req.max_tokens,
            "stream": false,
            "response_format": response_format
        })
    }

    fn resolve_api_key_from(
        api_key_env: &str,
        get: impl Fn(&str) -> Option<String>,
    ) -> Option<String> {
        if api_key_env.is_empty() {
            return None;
        }
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

    fn post_with_owned_bearer(
        &self,
        body: &Value,
        key: Option<&str>,
    ) -> Result<crate::http::HttpResponse, TransportError> {
        match key {
            Some(k) => {
                let bearer = format!("Bearer {k}");
                self.transport.post_json_with_headers(
                    &self.url("/chat/completions"),
                    body,
                    self.timeout(),
                    &[("Authorization", bearer.as_str())],
                )
            }
            None => self
                .transport
                .post_json(&self.url("/chat/completions"), body, self.timeout()),
        }
    }

    fn map_transport(&self, err: TransportError) -> BackendError {
        crate::error::from_transport("openai-compat", err, None)
    }

    fn looks_like_schema_reject(err: &TransportError) -> bool {
        match err {
            TransportError::Status { code, body } if matches!(*code, 400 | 415 | 422) => {
                let b = body.to_ascii_lowercase();
                b.contains("json_schema")
                    || b.contains("response_format")
                    || b.contains("not supported")
            }
            _ => false,
        }
    }
}

impl Backend for OpenAiCompatBackend {
    fn id(&self) -> &'static str {
        "openai-compat"
    }

    fn capabilities(&self) -> Capabilities {
        let loopback = self.settings.base_url.contains("127.0.0.1")
            || self.settings.base_url.contains("localhost");
        Capabilities {
            context_len: 128_000,
            structured_output: true,
            streaming: false,
            cost_tier: if loopback {
                CostTier::Local
            } else {
                CostTier::Cloud
            },
        }
    }

    fn health(&self) -> Health {
        match self.transport.get(&self.url("/models"), self.timeout()) {
            Ok(_) => Health {
                reachable: true,
                model_present: true,
                auth_configured: true,
                message: None,
            },
            Err(TransportError::Status {
                code: 401 | 403, ..
            }) => Health {
                reachable: true,
                model_present: true,
                auth_configured: self.api_key().is_some(),
                message: Some("endpoint requires an API key".into()),
            },
            Err(_) => Health {
                reachable: false,
                model_present: false,
                auth_configured: true,
                message: Some(format!(
                    "openai-compat unreachable at {}",
                    self.settings.base_url
                )),
            },
        }
    }

    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
        let key = self.api_key();
        let preferred = self
            .meta
            .get(&self.settings.base_url)
            .unwrap_or(StructuredFormat::JsonSchema);

        let started = Instant::now();
        let resp = match preferred {
            StructuredFormat::JsonObject => {
                let body =
                    Self::chat_request_body(&self.settings, req, StructuredFormat::JsonObject);
                self.post_with_owned_bearer(&body, key.as_deref())
                    .map_err(|e| self.map_transport(e))?
            }
            StructuredFormat::JsonSchema => {
                let body =
                    Self::chat_request_body(&self.settings, req, StructuredFormat::JsonSchema);
                match self.post_with_owned_bearer(&body, key.as_deref()) {
                    Ok(resp) => {
                        self.meta
                            .set(&self.settings.base_url, StructuredFormat::JsonSchema);
                        resp
                    }
                    Err(e) if Self::looks_like_schema_reject(&e) => {
                        let fallback = Self::chat_request_body(
                            &self.settings,
                            req,
                            StructuredFormat::JsonObject,
                        );
                        match self.post_with_owned_bearer(&fallback, key.as_deref()) {
                            Ok(resp) => {
                                self.meta
                                    .set(&self.settings.base_url, StructuredFormat::JsonObject);
                                resp
                            }
                            Err(e) => return Err(self.map_transport(e)),
                        }
                    }
                    Err(e) => return Err(self.map_transport(e)),
                }
            }
        };
        let latency_ms = started.elapsed().as_millis() as u64;
        parse_chat_response(&resp.body, &self.settings.model, latency_ms)
    }
}

fn parse_chat_response(
    body: &str,
    fallback_model: &str,
    latency_ms: u64,
) -> Result<TranslateResponse, BackendError> {
    let envelope: Value = serde_json::from_str(body).map_err(|e| {
        crate::error::from_bad_output("openai-compat", format!("openai-compat envelope: {e}"))
    })?;
    let content = envelope
        .pointer("/choices/0/message/content")
        .and_then(|c| c.as_str())
        .ok_or_else(|| {
            crate::error::from_bad_output(
                "openai-compat",
                "openai-compat response missing choices[0].message.content",
            )
        })?;
    let parsed = parse_response(content)
        .map_err(|e| crate::error::from_bad_output("openai-compat", e.to_string()))?;
    let confidence = parsed.candidates.first().map(|c| c.confidence);
    let usage = Usage {
        prompt_tokens: envelope
            .pointer("/usage/prompt_tokens")
            .and_then(|n| n.as_u64())
            .map(|n| n as u32),
        completion_tokens: envelope
            .pointer("/usage/completion_tokens")
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
        raw: content.to_string(),
        usage,
        backend_id: "openai-compat".into(),
        model,
        latency_ms,
        confidence,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::{Candidate, OutputSchema};

    use crate::backend::ErrorKind;
    use crate::http::HttpResponse;

    type HeaderLog = std::sync::Arc<Mutex<Vec<Vec<(String, String)>>>>;

    struct Scripted {
        reject_json_schema: bool,
        get: Mutex<Result<HttpResponse, TransportError>>,
        posts: std::sync::Arc<Mutex<Vec<Value>>>,
        headers: HeaderLog,
    }

    impl Transport for Scripted {
        fn get(&self, _url: &str, _timeout: Duration) -> Result<HttpResponse, TransportError> {
            self.get.lock().expect("get").clone()
        }

        fn post_json_with_headers(
            &self,
            _url: &str,
            body: &Value,
            _timeout: Duration,
            headers: &[(&str, &str)],
        ) -> Result<HttpResponse, TransportError> {
            self.posts.lock().expect("posts").push(body.clone());
            self.headers.lock().expect("headers").push(
                headers
                    .iter()
                    .map(|(k, v)| ((*k).to_string(), (*v).to_string()))
                    .collect(),
            );
            let ty = body
                .pointer("/response_format/type")
                .and_then(|t| t.as_str());
            if self.reject_json_schema && ty == Some("json_schema") {
                return Err(TransportError::Status {
                    code: 400,
                    body: "json_schema response_format is not supported".into(),
                });
            }
            let content = r#"{"commands":[{"command":"true","explanation":"noop","confidence":0.9}],"risk_notes":[],"assumptions":[]}"#;
            Ok(HttpResponse {
                status: 200,
                body: json!({
                    "model": "gpt-4o-mini",
                    "choices": [{
                        "message": { "role": "assistant", "content": content }
                    }],
                    "usage": { "prompt_tokens": 8, "completion_tokens": 3 }
                })
                .to_string(),
            })
        }
    }

    fn settings_lm() -> OpenAiCompatSettings {
        OpenAiCompatSettings {
            base_url: "http://127.0.0.1:1234/v1".into(),
            model: "local-model".into(),
            api_key_env: String::new(),
            timeout_ms: 5_000,
        }
    }

    fn sample_req() -> TranslateRequest {
        TranslateRequest {
            system: "sys".into(),
            user: "INTENT: run pg on 7000".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 256,
            temperature: 0.1,
            timeout_ms: 5_000,
        }
    }

    fn scripted(reject_json_schema: bool) -> Scripted {
        Scripted {
            reject_json_schema,
            get: Mutex::new(Ok(HttpResponse {
                status: 200,
                body: r#"{"data":[{"id":"local-model"}]}"#.into(),
            })),
            posts: Arc::new(Mutex::new(Vec::new())),
            headers: Arc::new(Mutex::new(Vec::new())),
        }
    }

    #[test]
    fn json_schema_body_has_named_schema() {
        let req = sample_req();
        let body = OpenAiCompatBackend::chat_request_body(
            &OpenAiCompatSettings::default(),
            &req,
            StructuredFormat::JsonSchema,
        );
        assert_eq!(body["response_format"]["type"], "json_schema");
        assert_eq!(
            body["response_format"]["json_schema"]["name"],
            req.schema.name
        );
        assert_eq!(
            body["response_format"]["json_schema"]["schema"],
            req.schema.json
        );
        assert_eq!(body["messages"][0]["role"], "system");
        assert_eq!(body["messages"][1]["content"], "INTENT: run pg on 7000");
    }

    #[test]
    fn json_object_body_embeds_schema_in_system() {
        let req = sample_req();
        let body = OpenAiCompatBackend::chat_request_body(
            &OpenAiCompatSettings::default(),
            &req,
            StructuredFormat::JsonObject,
        );
        assert_eq!(body["response_format"]["type"], "json_object");
        let sys = body["messages"][0]["content"].as_str().unwrap();
        assert!(sys.contains("Respond with a JSON object"));
        assert!(sys.contains("commands"));
    }

    #[test]
    fn probe_keeps_json_schema_when_accepted() {
        let t = scripted(false);
        let posts = Arc::clone(&t.posts);
        let meta = Arc::new(MetaCache::new());
        let b = OpenAiCompatBackend::with_transport(settings_lm(), t, None, Arc::clone(&meta));
        let resp = b.translate(&sample_req()).expect("ok");
        assert_eq!(resp.backend_id, "openai-compat");
        assert_eq!(resp.candidates[0].command, "true");
        assert_eq!(
            meta.get(&settings_lm().base_url),
            Some(StructuredFormat::JsonSchema)
        );
        b.translate(&sample_req()).expect("cached");
        let calls = posts.lock().expect("posts");
        assert_eq!(calls.len(), 2);
        assert!(
            calls
                .iter()
                .all(|c| c["response_format"]["type"] == "json_schema"),
            "{calls:?}"
        );
    }

    #[test]
    fn probe_downgrades_when_json_schema_rejected() {
        let t = scripted(true);
        let posts = Arc::clone(&t.posts);
        let meta = Arc::new(MetaCache::new());
        let b = OpenAiCompatBackend::with_transport(settings_lm(), t, None, Arc::clone(&meta));
        b.translate(&sample_req()).expect("downgrade ok");
        assert_eq!(
            meta.get(&settings_lm().base_url),
            Some(StructuredFormat::JsonObject)
        );
        b.translate(&sample_req()).expect("cached object");
        let calls = posts.lock().expect("posts");
        // first translate: schema reject + object success; second: object only
        assert_eq!(calls.len(), 3, "{calls:?}");
        assert_eq!(calls[0]["response_format"]["type"], "json_schema");
        assert_eq!(calls[1]["response_format"]["type"], "json_object");
        assert_eq!(calls[2]["response_format"]["type"], "json_object");
    }

    #[test]
    fn two_mock_servers_diverge_on_probe() {
        let meta_a = Arc::new(MetaCache::new());
        let meta_b = Arc::new(MetaCache::new());
        let a = OpenAiCompatBackend::with_transport(
            OpenAiCompatSettings {
                base_url: "http://127.0.0.1:1111/v1".into(),
                ..settings_lm()
            },
            scripted(false),
            None,
            meta_a,
        );
        let b = OpenAiCompatBackend::with_transport(
            OpenAiCompatSettings {
                base_url: "http://127.0.0.1:2222/v1".into(),
                ..settings_lm()
            },
            scripted(true),
            None,
            meta_b,
        );
        a.translate(&sample_req()).expect("schema server");
        b.translate(&sample_req()).expect("object server");
        assert_eq!(
            a.meta().get("http://127.0.0.1:1111/v1"),
            Some(StructuredFormat::JsonSchema)
        );
        assert_eq!(
            b.meta().get("http://127.0.0.1:2222/v1"),
            Some(StructuredFormat::JsonObject)
        );
    }

    #[test]
    fn lm_studio_no_key_omits_authorization() {
        let t = scripted(false);
        let headers = Arc::clone(&t.headers);
        let b =
            OpenAiCompatBackend::with_transport(settings_lm(), t, None, Arc::new(MetaCache::new()));
        b.translate(&sample_req()).expect("no key");
        let sent = headers.lock().expect("h");
        assert!(
            sent.iter()
                .all(|h| !h.iter().any(|(k, _)| k == "Authorization")),
            "LM Studio must not send Authorization: {sent:?}"
        );
    }

    #[test]
    fn bearer_sent_when_key_present() {
        let t = scripted(false);
        let headers = Arc::clone(&t.headers);
        let b = OpenAiCompatBackend::with_transport(
            OpenAiCompatSettings::default(),
            t,
            Some("sk-test".into()),
            Arc::new(MetaCache::new()),
        );
        b.translate(&sample_req()).expect("ok");
        let sent = headers.lock().expect("h");
        assert!(
            sent.iter().any(|h| h
                .iter()
                .any(|(k, v)| k == "Authorization" && v == "Bearer sk-test")),
            "{sent:?}"
        );
    }

    #[test]
    fn loopback_is_local_cost_tier() {
        let b = OpenAiCompatBackend::with_transport(
            settings_lm(),
            scripted(false),
            None,
            Arc::new(MetaCache::new()),
        );
        assert_eq!(b.capabilities().cost_tier, CostTier::Local);
        assert_eq!(b.id(), "openai-compat");
    }

    #[test]
    fn http_401_is_auth() {
        struct AuthFail;
        impl Transport for AuthFail {
            fn get(&self, _url: &str, _timeout: Duration) -> Result<HttpResponse, TransportError> {
                Err(TransportError::Unreachable("unused".into()))
            }
            fn post_json_with_headers(
                &self,
                _url: &str,
                _body: &Value,
                _timeout: Duration,
                _headers: &[(&str, &str)],
            ) -> Result<HttpResponse, TransportError> {
                Err(TransportError::Status {
                    code: 401,
                    body: "invalid api key".into(),
                })
            }
        }
        let b = OpenAiCompatBackend::with_transport(
            OpenAiCompatSettings::default(),
            AuthFail,
            Some("sk-bad".into()),
            Arc::new(MetaCache::new()),
        );
        let err = b.translate(&sample_req()).expect_err("401");
        assert_eq!(err.kind, ErrorKind::Auth);
        assert_eq!(err.backend, "openai-compat");
    }

    #[test]
    fn empty_api_key_env_never_reads_a_key() {
        let got =
            OpenAiCompatBackend::resolve_api_key_from("", |_| Some("sk-should-not-use".into()));
        assert!(got.is_none());
    }

    #[test]
    fn maps_chat_envelope() {
        let t = scripted(false);
        let b =
            OpenAiCompatBackend::with_transport(settings_lm(), t, None, Arc::new(MetaCache::new()));
        let resp = b.translate(&sample_req()).expect("ok");
        assert_eq!(
            resp.candidates,
            vec![Candidate {
                command: "true".into(),
                explanation: "noop".into(),
                confidence: 0.9,
            }]
        );
        assert_eq!(resp.usage.prompt_tokens, Some(8));
        assert_eq!(resp.model, "gpt-4o-mini");
    }
}
