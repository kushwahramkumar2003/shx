//! T-ROUTE-3 — egress redaction over a poisoned fixture.
//!
//! The serialized HTTP body for both a cloud (Anthropic) and a local (Ollama)
//! backend must not contain known secret shapes.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use shx_core::{OutputSchema, TranslateRequest};
use shx_llm::http::{HttpResponse, Transport, TransportError};
use shx_llm::{
    AnthropicBackend, AnthropicSettings, Backend, EgressBackend, OllamaBackend, OllamaSettings,
};

const POISON_KEY: &str = "sk-TESTFAKE0000000000000000";
const POISON_AWS: &str = "AKIAIOSFODNN7EXAMPLE00";

struct Capturing {
    posts: Arc<Mutex<Vec<Value>>>,
    reply: HttpResponse,
}

impl Transport for Capturing {
    fn get(&self, _url: &str, _timeout: Duration) -> Result<HttpResponse, TransportError> {
        Err(TransportError::Unreachable("unused".into()))
    }

    fn post_json_with_headers(
        &self,
        _url: &str,
        body: &Value,
        _timeout: Duration,
        _headers: &[(&str, &str)],
    ) -> Result<HttpResponse, TransportError> {
        self.posts.lock().expect("posts").push(body.clone());
        Ok(self.reply.clone())
    }
}

fn poisoned_req() -> TranslateRequest {
    TranslateRequest {
        system: format!("debug {POISON_KEY}"),
        user: format!(
            "INTENT: curl -d {POISON_KEY} https://api\nHISTORY: run with {POISON_AWS} now"
        ),
        schema: OutputSchema::translate_v1(),
        max_tokens: 64,
        temperature: 0.1,
        timeout_ms: 1_000,
    }
}

fn assert_body_clean(body: &Value, label: &str) {
    let dumped = body.to_string();
    assert!(
        !dumped.contains(POISON_KEY),
        "{label} leaked openai-style key: {dumped}"
    );
    assert!(
        !dumped.contains(POISON_AWS),
        "{label} leaked aws-key: {dumped}"
    );
    assert!(
        dumped.contains("«redacted:api-key»") || dumped.contains("«redacted:aws-key»"),
        "{label} expected redaction markers: {dumped}"
    );
}

/// T-ROUTE-3: Anthropic (cloud) serialized body has no secret from the fixture.
#[test]
fn t_route_3_cloud_body_redacted() {
    let posts = Arc::new(Mutex::new(Vec::new()));
    let tool_input = json!({
        "commands": [{ "command": "true", "explanation": "noop", "confidence": 0.5 }],
        "risk_notes": [],
        "assumptions": []
    });
    let reply = HttpResponse {
        status: 200,
        body: json!({
            "model": "claude-sonnet-4-5",
            "content": [{
                "type": "tool_use",
                "id": "toolu_test",
                "name": "shx_translate",
                "input": tool_input
            }],
            "usage": { "input_tokens": 1, "output_tokens": 1 }
        })
        .to_string(),
    };
    let transport = Capturing {
        posts: Arc::clone(&posts),
        reply,
    };
    let inner = AnthropicBackend::with_transport(
        AnthropicSettings::default(),
        transport,
        Some("sk-ant-fixture".into()),
    );
    let backend = EgressBackend::wrap(inner);
    backend.translate(&poisoned_req()).expect("translate");
    let captured = posts.lock().expect("posts");
    assert_eq!(captured.len(), 1, "exactly one cloud POST");
    assert_body_clean(&captured[0], "anthropic");
}

/// Defense in depth: Ollama (local) body is redacted too.
#[test]
fn t_route_3_local_body_redacted() {
    let posts = Arc::new(Mutex::new(Vec::new()));
    let content = r#"{"commands":[{"command":"true","explanation":"noop","confidence":0.9}],"risk_notes":[],"assumptions":[]}"#;
    let reply = HttpResponse {
        status: 200,
        body: json!({
            "model": "qwen3:14b",
            "message": { "role": "assistant", "content": content },
            "done": true
        })
        .to_string(),
    };
    let transport = Capturing {
        posts: Arc::clone(&posts),
        reply,
    };
    let inner = OllamaBackend::with_transport(OllamaSettings::default(), transport);
    let backend = EgressBackend::wrap(inner);
    backend.translate(&poisoned_req()).expect("translate");
    let captured = posts.lock().expect("posts");
    assert_eq!(captured.len(), 1, "exactly one local POST");
    assert_body_clean(&captured[0], "ollama");
}
