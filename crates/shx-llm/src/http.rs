//! Blocking HTTP via `ureq` + rustls (ADR-006). No tokio, no reqwest, no OpenSSL.

use std::io::{BufRead, BufReader};
use std::time::Duration;

use serde_json::Value;

/// Outcome of one HTTP call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Response body as UTF-8 (lossy if not).
    pub body: String,
}

/// Transport failure before a usable HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TransportError {
    /// Nothing listening / DNS / connection refused.
    Unreachable(String),
    /// Deadline exceeded.
    Timeout(String),
    /// HTTP status that is not 2xx.
    Status {
        /// Status code.
        code: u16,
        /// Response body.
        body: String,
    },
    /// Other I/O or protocol error.
    Other(String),
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unreachable(s) => write!(f, "unreachable: {s}"),
            Self::Timeout(s) => write!(f, "timeout: {s}"),
            Self::Status { code, body } => write!(f, "HTTP {code}: {body}"),
            Self::Other(s) => write!(f, "{s}"),
        }
    }
}

impl std::error::Error for TransportError {}

/// Blocking JSON GET/POST. Implemented by [`UreqTransport`] and test doubles.
pub trait Transport: Send + Sync {
    /// GET `url`, expecting a JSON/text body.
    fn get(&self, url: &str, timeout: Duration) -> Result<HttpResponse, TransportError>;

    /// POST JSON `body` to `url` with no extra headers.
    fn post_json(
        &self,
        url: &str,
        body: &Value,
        timeout: Duration,
    ) -> Result<HttpResponse, TransportError> {
        self.post_json_with_headers(url, body, timeout, &[])
    }

    /// POST JSON `body` to `url` with extra headers (`x-api-key`, …).
    fn post_json_with_headers(
        &self,
        url: &str,
        body: &Value,
        timeout: Duration,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, TransportError>;

    /// POST JSON and deliver each non-empty response line.
    ///
    /// The default reads the whole body via [`Self::post_json`], so `timeout`
    /// is still the overall deadline. [`UreqTransport`] overrides this and
    /// treats `timeout` as the idle gap between socket reads, which lets a
    /// streaming body run longer than one chunk interval.
    fn post_json_lines(
        &self,
        url: &str,
        body: &Value,
        timeout: Duration,
        on_line: &mut dyn FnMut(&str) -> Result<(), TransportError>,
    ) -> Result<(), TransportError> {
        let resp = self.post_json(url, body, timeout)?;
        for line in resp.body.lines() {
            if line.is_empty() {
                continue;
            }
            on_line(line)?;
        }
        Ok(())
    }
}

/// Production transport.
#[derive(Debug, Default, Clone, Copy)]
pub struct UreqTransport;

impl Transport for UreqTransport {
    fn get(&self, url: &str, timeout: Duration) -> Result<HttpResponse, TransportError> {
        let resp = ureq::get(url).timeout(timeout).call().map_err(map_ureq)?;
        read_response(resp)
    }

    fn post_json_with_headers(
        &self,
        url: &str,
        body: &Value,
        timeout: Duration,
        headers: &[(&str, &str)],
    ) -> Result<HttpResponse, TransportError> {
        let mut req = ureq::post(url).timeout(timeout);
        for (k, v) in headers {
            req = req.set(k, v);
        }
        let resp = req.send_json(body.clone()).map_err(map_ureq)?;
        read_response(resp)
    }

    fn post_json_lines(
        &self,
        url: &str,
        body: &Value,
        timeout: Duration,
        on_line: &mut dyn FnMut(&str) -> Result<(), TransportError>,
    ) -> Result<(), TransportError> {
        // Idle read timeout, not an overall deadline. Each successful read
        // starts the wait again, so a model that keeps emitting tokens is
        // not cut off at `timeout`.
        let agent = ureq::AgentBuilder::new()
            .timeout_connect(timeout)
            .timeout_read(timeout)
            .timeout_write(timeout)
            .build();
        let resp = agent.post(url).send_json(body.clone()).map_err(map_ureq)?;
        let status = resp.status();
        if !(200..300).contains(&status) {
            let body = resp.into_string().unwrap_or_default();
            return Err(TransportError::Status { code: status, body });
        }
        let mut reader = BufReader::new(resp.into_reader());
        let mut line = String::new();
        loop {
            line.clear();
            let n = reader.read_line(&mut line).map_err(map_io)?;
            if n == 0 {
                break;
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                continue;
            }
            on_line(trimmed)?;
        }
        Ok(())
    }
}

fn map_io(err: std::io::Error) -> TransportError {
    let msg = err.to_string();
    if err.kind() == std::io::ErrorKind::TimedOut || is_timeout(&msg) {
        TransportError::Timeout(msg)
    } else {
        TransportError::Other(msg)
    }
}

fn read_response(resp: ureq::Response) -> Result<HttpResponse, TransportError> {
    let status = resp.status();
    let body = resp
        .into_string()
        .map_err(|e| TransportError::Other(e.to_string()))?;
    if (200..300).contains(&status) {
        Ok(HttpResponse { status, body })
    } else {
        Err(TransportError::Status { code: status, body })
    }
}

fn map_ureq(err: ureq::Error) -> TransportError {
    match err {
        ureq::Error::Status(code, resp) => {
            let body = resp.into_string().unwrap_or_default();
            TransportError::Status { code, body }
        }
        ureq::Error::Transport(t) => {
            let msg = t.to_string();
            match t.kind() {
                ureq::ErrorKind::Io if is_timeout(&msg) => TransportError::Timeout(msg),
                ureq::ErrorKind::ConnectionFailed | ureq::ErrorKind::Dns | ureq::ErrorKind::Io => {
                    TransportError::Unreachable(msg)
                }
                _ if is_timeout(&msg) => TransportError::Timeout(msg),
                _ => TransportError::Other(msg),
            }
        }
    }
}

fn is_timeout(msg: &str) -> bool {
    let m = msg.to_ascii_lowercase();
    m.contains("timed out") || m.contains("timeout")
}
