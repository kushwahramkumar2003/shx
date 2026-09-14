//! Local-first backend router (docs/06-BACKENDS.md §3, ADR-012).
//!
//! At most one escalation. Notices are returned on the trace (the CLI prints
//! them on stderr). `mode=local` never calls cloud.

use std::time::Instant;

use shx_core::{TranslateRequest, TranslateResponse};

use crate::backend::{Backend, BackendError, ErrorKind};

/// How the router picks backends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteMode {
    /// Local only; never egress.
    Local,
    /// Cloud only.
    Cloud,
    /// Local first; escalate once on failure / low confidence / slowness.
    LocalFirst,
}

impl RouteMode {
    /// Config spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Local => "local",
            Self::Cloud => "cloud",
            Self::LocalFirst => "local-first",
        }
    }
}

/// Knobs from `[backend]`.
#[derive(Debug, Clone, Copy)]
pub struct RouterConfig {
    /// Selection policy.
    pub mode: RouteMode,
    /// Escalate when reported confidence is below this (default 0.6).
    pub escalate_below_confidence: f32,
    /// Escalate when local wall time exceeds this even if it succeeded (ms).
    pub local_slow_ms: u64,
    /// Skip local on multi-clause intents (ADR-009; default off).
    pub complexity_skip_local: bool,
}

impl Default for RouterConfig {
    fn default() -> Self {
        Self {
            mode: RouteMode::LocalFirst,
            escalate_below_confidence: 0.6,
            local_slow_ms: 8_000,
            complexity_skip_local: false,
        }
    }
}

/// What happened during routing (`--why`, `--json` `escalated_from`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteTrace {
    /// Policy used.
    pub mode: RouteMode,
    /// Backend ids that were invoked, in order.
    pub tried: Vec<String>,
    /// Backend that produced the returned response.
    pub chosen: String,
    /// Local id if the cloud answered after an escalation.
    pub escalated_from: Option<String>,
    /// Why we left local (`timeout`, `low-confidence`, …).
    pub reason: Option<String>,
    /// Human line for stderr, e.g. `escalated to anthropic (local: timeout)`.
    pub notice: Option<String>,
}

/// Successful routed translation.
#[derive(Debug, Clone)]
pub struct RoutedTranslate {
    /// Provider response (already schema-validated by the backend).
    pub response: TranslateResponse,
    /// Routing explainability.
    pub trace: RouteTrace,
}

/// Local-first (or forced) router over two [`Backend`]s.
pub struct BackendRouter {
    local: Box<dyn Backend>,
    cloud: Option<Box<dyn Backend>>,
    config: RouterConfig,
}

impl BackendRouter {
    /// `cloud` may be `None` when unconfigured; local-first then cannot escalate.
    pub fn new(
        local: Box<dyn Backend>,
        cloud: Option<Box<dyn Backend>>,
        config: RouterConfig,
    ) -> Self {
        Self {
            local,
            cloud,
            config,
        }
    }

    /// Pick a backend and translate. At most one cloud call.
    pub fn route(&self, req: &TranslateRequest) -> Result<RoutedTranslate, BackendError> {
        match self.config.mode {
            RouteMode::Cloud => self.cloud_only(req, None),
            RouteMode::Local => self.local_only(req),
            RouteMode::LocalFirst => {
                if self.config.complexity_skip_local
                    && looks_complex(&req.user)
                    && self.cloud.is_some()
                {
                    return self.cloud_only(req, Some("complexity"));
                }
                self.local_then_maybe_escalate(req)
            }
        }
    }

    fn local_only(&self, req: &TranslateRequest) -> Result<RoutedTranslate, BackendError> {
        let resp = self.local.translate(req)?;
        Ok(ok_local(
            resp,
            self.local.id(),
            RouteMode::Local,
            vec![self.local.id().to_string()],
        ))
    }

    fn cloud_only(
        &self,
        req: &TranslateRequest,
        reason: Option<&str>,
    ) -> Result<RoutedTranslate, BackendError> {
        let cloud = self.cloud.as_ref().ok_or_else(cloud_unconfigured)?;
        let resp = cloud.translate(req)?;
        let notice = reason.map(|r| format!("escalated to {} (local: {r})", cloud.id()));
        Ok(RoutedTranslate {
            response: resp,
            trace: RouteTrace {
                mode: self.config.mode,
                tried: vec![cloud.id().to_string()],
                chosen: cloud.id().to_string(),
                escalated_from: reason.map(|_| self.local.id().to_string()),
                reason: reason.map(str::to_string),
                notice,
            },
        })
    }

    fn local_then_maybe_escalate(
        &self,
        req: &TranslateRequest,
    ) -> Result<RoutedTranslate, BackendError> {
        let started = Instant::now();
        let local_id = self.local.id();
        match self.local.translate(req) {
            Ok(resp) => {
                let wall = started.elapsed().as_millis() as u64;
                let latency = wall.max(resp.latency_ms);
                let conf = resp.confidence.unwrap_or(1.0);
                let reason = if conf < self.config.escalate_below_confidence {
                    Some("low-confidence")
                } else if latency > self.config.local_slow_ms {
                    Some("slow")
                } else {
                    None
                };
                match reason {
                    Some(reason) => self.escalate_or_keep(req, Ok(resp), local_id, reason),
                    None => Ok(ok_local(
                        resp,
                        local_id,
                        RouteMode::LocalFirst,
                        vec![local_id.to_string()],
                    )),
                }
            }
            Err(err) if should_escalate(&err) => {
                let reason = reason_from_error(&err);
                self.escalate_or_keep(req, Err(err), local_id, &reason)
            }
            Err(err) => Err(err),
        }
    }

    fn escalate_or_keep(
        &self,
        req: &TranslateRequest,
        local: Result<TranslateResponse, BackendError>,
        local_id: &'static str,
        reason: &str,
    ) -> Result<RoutedTranslate, BackendError> {
        let Some(cloud) = self.cloud.as_ref() else {
            return match local {
                Ok(resp) => Ok(ok_local(
                    resp,
                    local_id,
                    RouteMode::LocalFirst,
                    vec![local_id.to_string()],
                )),
                Err(e) => Err(e),
            };
        };
        match cloud.translate(req) {
            Ok(resp) => Ok(RoutedTranslate {
                response: resp,
                trace: RouteTrace {
                    mode: RouteMode::LocalFirst,
                    tried: vec![local_id.to_string(), cloud.id().to_string()],
                    chosen: cloud.id().to_string(),
                    escalated_from: Some(local_id.to_string()),
                    reason: Some(reason.to_string()),
                    notice: Some(format!("escalated to {} (local: {reason})", cloud.id())),
                },
            }),
            Err(_cloud_err) => match local {
                Ok(resp) => Ok(ok_local(
                    resp,
                    local_id,
                    RouteMode::LocalFirst,
                    vec![local_id.to_string(), cloud.id().to_string()],
                )),
                Err(local_err) => Err(local_err),
            },
        }
    }
}

fn ok_local(
    resp: TranslateResponse,
    local_id: &'static str,
    mode: RouteMode,
    tried: Vec<String>,
) -> RoutedTranslate {
    RoutedTranslate {
        response: resp,
        trace: RouteTrace {
            mode,
            tried,
            chosen: local_id.to_string(),
            escalated_from: None,
            reason: None,
            notice: None,
        },
    }
}

fn should_escalate(err: &BackendError) -> bool {
    err.retryable
        || matches!(
            err.kind,
            ErrorKind::Timeout | ErrorKind::ModelMissing | ErrorKind::BadOutput
        )
}

fn reason_from_error(err: &BackendError) -> String {
    match err.kind {
        ErrorKind::Timeout => "timeout".into(),
        ErrorKind::Unreachable => "unreachable".into(),
        ErrorKind::ModelMissing => "model_missing".into(),
        ErrorKind::BadOutput => "bad_output".into(),
        ErrorKind::RateLimit => "rate_limit".into(),
        ErrorKind::Server => "server".into(),
        ErrorKind::Auth => "auth".into(),
    }
}

fn cloud_unconfigured() -> BackendError {
    BackendError {
        kind: ErrorKind::Auth,
        backend: "router",
        retryable: false,
        detail: "cloud backend unconfigured; set the env var named in backend.cloud.api_key_env"
            .into(),
    }
}

fn looks_complex(user: &str) -> bool {
    let t = user.to_ascii_lowercase();
    t.contains(" and then ") || (t.contains(" then ") && t.contains("&&"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use shx_core::OutputSchema;

    use crate::backend::{Capabilities, Health};
    use crate::mock::MockBackend;

    struct Named {
        id: &'static str,
        inner: MockBackend,
        calls: Arc<AtomicUsize>,
        latency_ms: u64,
    }

    impl Backend for Named {
        fn id(&self) -> &'static str {
            self.id
        }

        fn capabilities(&self) -> Capabilities {
            self.inner.capabilities()
        }

        fn health(&self) -> Health {
            self.inner.health()
        }

        fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            match self.inner.translate(req) {
                Ok(mut r) => {
                    r.backend_id = self.id.to_string();
                    r.latency_ms = r.latency_ms.max(self.latency_ms);
                    Ok(r)
                }
                Err(mut e) => {
                    e.backend = self.id;
                    Err(e)
                }
            }
        }
    }

    fn req(intent: &str) -> TranslateRequest {
        TranslateRequest {
            system: String::new(),
            user: format!("INTENT: {intent}"),
            schema: OutputSchema::translate_v1(),
            max_tokens: 64,
            temperature: 0.1,
            timeout_ms: 1_000,
        }
    }

    fn local_cloud(
        local: MockBackend,
        cloud: MockBackend,
        cfg: RouterConfig,
    ) -> (BackendRouter, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let lc = Arc::new(AtomicUsize::new(0));
        let cc = Arc::new(AtomicUsize::new(0));
        let router = BackendRouter::new(
            Box::new(Named {
                id: "ollama",
                inner: local,
                calls: Arc::clone(&lc),
                latency_ms: 0,
            }),
            Some(Box::new(Named {
                id: "anthropic",
                inner: cloud,
                calls: Arc::clone(&cc),
                latency_ms: 0,
            })),
            cfg,
        );
        (router, lc, cc)
    }

    /// T-ROUTE-1: local Timeout → exactly one cloud call + escalation notice.
    #[test]
    fn t_route_1_timeout_escalates_once() {
        let mut local = MockBackend::empty();
        local.script_fault("boom", ErrorKind::Timeout);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("boom", "true", "cloud", 0.9);
        let cfg = RouterConfig {
            mode: RouteMode::LocalFirst,
            ..RouterConfig::default()
        };
        let (router, lc, cc) = local_cloud(local, cloud, cfg);
        let out = router.route(&req("boom")).expect("cloud");
        assert_eq!(lc.load(Ordering::SeqCst), 1);
        assert_eq!(cc.load(Ordering::SeqCst), 1);
        assert_eq!(out.response.backend_id, "anthropic");
        assert_eq!(out.trace.escalated_from.as_deref(), Some("ollama"));
        assert_eq!(
            out.trace.notice.as_deref(),
            Some("escalated to anthropic (local: timeout)")
        );
        assert_eq!(out.trace.tried, ["ollama", "anthropic"]);
    }

    /// T-ROUTE-2: `mode=local` → zero cloud calls on any local outcome.
    #[test]
    fn t_route_2_local_mode_never_calls_cloud() {
        let mut local = MockBackend::empty();
        local.script_fault("boom", ErrorKind::Timeout);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("boom", "true", "cloud", 0.9);
        let cfg = RouterConfig {
            mode: RouteMode::Local,
            ..RouterConfig::default()
        };
        let (router, lc, cc) = local_cloud(local, cloud, cfg);
        let err = router.route(&req("boom")).expect_err("local timeout");
        assert_eq!(err.kind, ErrorKind::Timeout);
        assert_eq!(lc.load(Ordering::SeqCst), 1);
        assert_eq!(cc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn t_route_2_local_success_never_calls_cloud() {
        let mut local = MockBackend::empty();
        local.script_ok("ok", "echo hi", "local", 0.95);
        let cloud = MockBackend::empty();
        let cfg = RouterConfig {
            mode: RouteMode::Local,
            ..RouterConfig::default()
        };
        let (router, _, cc) = local_cloud(local, cloud, cfg);
        let out = router.route(&req("ok")).expect("ok");
        assert_eq!(out.response.candidates[0].command, "echo hi");
        assert_eq!(cc.load(Ordering::SeqCst), 0);
        assert!(out.trace.notice.is_none());
    }

    #[test]
    fn low_confidence_escalates() {
        let mut local = MockBackend::empty();
        local.script_ok("ambig", "echo maybe", "local", 0.4);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("ambig", "echo sure", "cloud", 0.9);
        let cfg = RouterConfig {
            mode: RouteMode::LocalFirst,
            escalate_below_confidence: 0.6,
            ..RouterConfig::default()
        };
        let (router, _, cc) = local_cloud(local, cloud, cfg);
        let out = router.route(&req("ambig")).expect("esc");
        assert_eq!(cc.load(Ordering::SeqCst), 1);
        assert_eq!(out.response.candidates[0].command, "echo sure");
        assert_eq!(out.trace.reason.as_deref(), Some("low-confidence"));
        assert_eq!(
            out.trace.notice.as_deref(),
            Some("escalated to anthropic (local: low-confidence)")
        );
    }

    #[test]
    fn high_confidence_stays_local() {
        let mut local = MockBackend::empty();
        local.script_ok("ok", "echo hi", "local", 0.95);
        let cloud = MockBackend::empty();
        let cfg = RouterConfig::default();
        let (router, _, cc) = local_cloud(local, cloud, cfg);
        let out = router.route(&req("ok")).expect("local");
        assert_eq!(cc.load(Ordering::SeqCst), 0);
        assert_eq!(out.trace.chosen, "ollama");
        assert!(out.trace.notice.is_none());
    }

    #[test]
    fn auth_does_not_escalate() {
        let mut local = MockBackend::empty();
        local.script_fault("secret", ErrorKind::Auth);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("secret", "true", "cloud", 0.9);
        let (router, _, cc) = local_cloud(local, cloud, RouterConfig::default());
        let err = router.route(&req("secret")).expect_err("auth");
        assert_eq!(err.kind, ErrorKind::Auth);
        assert_eq!(cc.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn slow_local_escalates() {
        let mut local = MockBackend::empty();
        local.script_ok("ok", "echo hi", "local", 0.95);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("ok", "echo cloud", "cloud", 0.9);
        let lc = Arc::new(AtomicUsize::new(0));
        let cc = Arc::new(AtomicUsize::new(0));
        let router = BackendRouter::new(
            Box::new(Named {
                id: "ollama",
                inner: local,
                calls: Arc::clone(&lc),
                latency_ms: 9_000,
            }),
            Some(Box::new(Named {
                id: "anthropic",
                inner: cloud,
                calls: Arc::clone(&cc),
                latency_ms: 0,
            })),
            RouterConfig {
                local_slow_ms: 8_000,
                ..RouterConfig::default()
            },
        );
        let out = router.route(&req("ok")).expect("esc");
        assert_eq!(cc.load(Ordering::SeqCst), 1);
        assert_eq!(out.trace.reason.as_deref(), Some("slow"));
    }

    #[test]
    fn cloud_mode_skips_local() {
        let mut local = MockBackend::empty();
        local.script_ok("ok", "echo local", "local", 0.9);
        let mut cloud = MockBackend::empty();
        cloud.script_ok("ok", "echo cloud", "cloud", 0.9);
        let cfg = RouterConfig {
            mode: RouteMode::Cloud,
            ..RouterConfig::default()
        };
        let (router, lc, cc) = local_cloud(local, cloud, cfg);
        let out = router.route(&req("ok")).expect("cloud");
        assert_eq!(lc.load(Ordering::SeqCst), 0);
        assert_eq!(cc.load(Ordering::SeqCst), 1);
        assert_eq!(out.response.candidates[0].command, "echo cloud");
    }

    #[test]
    fn local_first_without_cloud_returns_local_error() {
        let mut local = MockBackend::empty();
        local.script_fault("boom", ErrorKind::Timeout);
        let router = BackendRouter::new(
            Box::new(Named {
                id: "ollama",
                inner: local,
                calls: Arc::new(AtomicUsize::new(0)),
                latency_ms: 0,
            }),
            None,
            RouterConfig::default(),
        );
        let err = router.route(&req("boom")).expect_err("no cloud");
        assert_eq!(err.kind, ErrorKind::Timeout);
    }
}
