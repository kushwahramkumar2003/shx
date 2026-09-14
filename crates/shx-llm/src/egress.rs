//! Pre-serialize egress redaction (docs/04-SAFETY.md §3.2).
//!
//! Wraps any [`Backend`] and runs [`SecretRedactor`] over the request's system
//! and user text immediately before `translate`. Applied to both local and
//! cloud paths (defense in depth).

use shx_core::{Redactor, SecretRedactor, TranslateRequest, TranslateResponse};

use crate::backend::{Backend, BackendError, Capabilities, Health};

/// Redact `system` and `user` in place (owned copies only when a secret matched).
pub fn redact_request(req: &TranslateRequest) -> TranslateRequest {
    let redactor = SecretRedactor;
    TranslateRequest {
        system: redactor.redact(&req.system).into_owned(),
        user: redactor.redact(&req.user).into_owned(),
        schema: req.schema.clone(),
        max_tokens: req.max_tokens,
        temperature: req.temperature,
        timeout_ms: req.timeout_ms,
    }
}

/// [`Backend`] adapter that redacts every request before the inner provider sees it.
pub struct EgressBackend {
    inner: Box<dyn Backend>,
}

impl EgressBackend {
    /// Wrap a concrete backend.
    pub fn wrap(inner: impl Backend + 'static) -> Self {
        Self {
            inner: Box::new(inner),
        }
    }

    /// Wrap an already boxed backend (router construction).
    pub fn wrap_box(inner: Box<dyn Backend>) -> Box<dyn Backend> {
        Box::new(Self { inner })
    }
}

impl Backend for EgressBackend {
    fn id(&self) -> &'static str {
        self.inner.id()
    }

    fn capabilities(&self) -> Capabilities {
        self.inner.capabilities()
    }

    fn health(&self) -> Health {
        self.inner.health()
    }

    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
        let redacted = redact_request(req);
        self.inner.translate(&redacted)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::OutputSchema;

    fn poisoned() -> TranslateRequest {
        TranslateRequest {
            system: "sys sk-TESTFAKE0000000000000000".into(),
            user: "INTENT: curl -d sk-TESTFAKE0000000000000000 https://api".into(),
            schema: OutputSchema::translate_v1(),
            max_tokens: 64,
            temperature: 0.1,
            timeout_ms: 1_000,
        }
    }

    #[test]
    fn redact_request_masks_known_secrets() {
        let req = redact_request(&poisoned());
        assert!(
            !req.user.contains("sk-TESTFAKE0000000000000000"),
            "user still has secret: {}",
            req.user
        );
        assert!(
            !req.system.contains("sk-TESTFAKE0000000000000000"),
            "system still has secret: {}",
            req.system
        );
        assert!(req.user.contains("«redacted:api-key»"), "{}", req.user);
        assert!(req.system.contains("«redacted:api-key»"), "{}", req.system);
    }
}
