//! Normalize provider failures into [`BackendError`] (docs/06-BACKENDS.md §1).
//!
//! The router keys off [`ErrorKind`], never provider-specific strings.

use crate::backend::{BackendError, ErrorKind};
use crate::http::TransportError;

/// Whether the router may retry or escalate this kind.
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

/// Map a transport/HTTP failure onto [`BackendError`].
///
/// `pull_hint` is appended for model-missing (e.g. `run: ollama pull qwen3:14b`).
pub fn from_transport(
    backend: &'static str,
    err: TransportError,
    pull_hint: Option<&str>,
) -> BackendError {
    match err {
        TransportError::Unreachable(s) => fail(backend, ErrorKind::Unreachable, s),
        TransportError::Timeout(s) => fail(backend, ErrorKind::Timeout, s),
        TransportError::Status { code, body } => from_http_status(backend, code, &body, pull_hint),
        TransportError::Other(s) => fail(backend, ErrorKind::Server, s),
    }
}

/// Map an HTTP status + body.
pub fn from_http_status(
    backend: &'static str,
    code: u16,
    body: &str,
    pull_hint: Option<&str>,
) -> BackendError {
    let lower = body.to_ascii_lowercase();
    let looks_missing = lower.contains("not found") || lower.contains("try pulling");
    match code {
        401 | 403 => fail(backend, ErrorKind::Auth, body),
        404 if looks_missing || pull_hint.is_some() => {
            fail(backend, ErrorKind::ModelMissing, with_hint(body, pull_hint))
        }
        404 => fail(backend, ErrorKind::Unreachable, body),
        429 => fail(backend, ErrorKind::RateLimit, body),
        500..=599 => fail(backend, ErrorKind::Server, body),
        _ if looks_missing => fail(backend, ErrorKind::ModelMissing, with_hint(body, pull_hint)),
        _ => fail(backend, ErrorKind::Server, format!("HTTP {code}: {body}")),
    }
}

/// Schema/parse failure → `BadOutput`.
pub fn from_bad_output(backend: &'static str, detail: impl Into<String>) -> BackendError {
    fail(backend, ErrorKind::BadOutput, detail)
}

fn with_hint(body: &str, hint: Option<&str>) -> String {
    match hint {
        Some(h) if !body.contains(h) => format!("{body}; {h}"),
        _ => body.to_string(),
    }
}

fn fail(backend: &'static str, kind: ErrorKind, detail: impl Into<String>) -> BackendError {
    BackendError {
        kind,
        backend,
        retryable: retryable(kind),
        detail: detail.into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Case {
        name: &'static str,
        err: TransportError,
        kind: ErrorKind,
        retryable: bool,
        hint_substr: Option<&'static str>,
    }

    #[test]
    fn provider_conditions_map_to_kind_and_retryable() {
        let pull = "run: ollama pull qwen3:14b";
        let cases = [
            Case {
                name: "connection refused",
                err: TransportError::Unreachable("connection refused".into()),
                kind: ErrorKind::Unreachable,
                retryable: true,
                hint_substr: None,
            },
            Case {
                name: "timeout",
                err: TransportError::Timeout("timed out".into()),
                kind: ErrorKind::Timeout,
                retryable: true,
                hint_substr: None,
            },
            Case {
                name: "401",
                err: TransportError::Status {
                    code: 401,
                    body: "unauthorized".into(),
                },
                kind: ErrorKind::Auth,
                retryable: false,
                hint_substr: None,
            },
            Case {
                name: "403",
                err: TransportError::Status {
                    code: 403,
                    body: "forbidden".into(),
                },
                kind: ErrorKind::Auth,
                retryable: false,
                hint_substr: None,
            },
            Case {
                name: "model missing 404",
                err: TransportError::Status {
                    code: 404,
                    body: "model 'qwen3:14b' not found, try pulling it first".into(),
                },
                kind: ErrorKind::ModelMissing,
                retryable: false,
                hint_substr: Some(pull),
            },
            Case {
                name: "429",
                err: TransportError::Status {
                    code: 429,
                    body: "rate limited".into(),
                },
                kind: ErrorKind::RateLimit,
                retryable: true,
                hint_substr: None,
            },
            Case {
                name: "500",
                err: TransportError::Status {
                    code: 500,
                    body: "internal".into(),
                },
                kind: ErrorKind::Server,
                retryable: true,
                hint_substr: None,
            },
            Case {
                name: "502",
                err: TransportError::Status {
                    code: 502,
                    body: "bad gateway".into(),
                },
                kind: ErrorKind::Server,
                retryable: true,
                hint_substr: None,
            },
        ];
        for c in cases {
            let err = from_transport("ollama", c.err, Some(pull));
            assert_eq!(err.kind, c.kind, "{}", c.name);
            assert_eq!(err.retryable, c.retryable, "{}", c.name);
            assert_eq!(err.backend, "ollama", "{}", c.name);
            if let Some(s) = c.hint_substr {
                assert!(err.detail.contains(s), "{}: {}", c.name, err.detail);
            }
        }

        let bad = from_bad_output("ollama", "truncated JSON");
        assert_eq!(bad.kind, ErrorKind::BadOutput);
        assert!(bad.retryable);
    }
}
