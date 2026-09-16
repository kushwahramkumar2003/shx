//! Translate pipeline: mock backend, optional SQLite write with redaction.

use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::time::Instant;

use shx_config::{BackendMode, Config, FlagOverrides};
use shx_core::{
    Candidate, ContextBundle, EnvInfo, ForceBackend, Intent, IntentFlags, Interaction, Profile,
    PromptBuilder, RiskAssessment, RiskClassifier, RiskLevel, SecretRedactor,
};
use shx_llm::{
    AnthropicBackend, AnthropicSettings, Backend, BackendError, BackendRouter, EgressBackend,
    MockBackend, OllamaBackend, OllamaSettings, OpenAiCompatBackend, OpenAiCompatSettings,
    RouteMode, RouterConfig,
};
use shx_memory::{CacheQuery, ContextBudget, ContextBuilder, MemoryStore, SqliteStore, paths};

use crate::Cli;

/// Successful translation ready to render.
#[derive(Debug, Clone)]
pub struct TranslateOut {
    /// Original intent text.
    pub input: String,
    /// Candidates to print (already truncated to `-n`).
    pub candidates: Vec<Candidate>,
    /// Classifier result for the (possibly truncated) first command.
    pub risk: RiskAssessment,
    /// Backend id that answered.
    pub backend_id: String,
    /// Model id.
    pub model: String,
    /// End-to-end latency.
    pub latency_ms: u64,
    /// Fast-path cache (always false in T-006).
    pub from_cache: bool,
    /// Config unknown-key warnings (stderr).
    pub warnings: Vec<String>,
    /// Raw model text (`--verbose`).
    pub raw: String,
    /// Data for `--why` (stderr only).
    pub why: WhyInfo,
    /// Set when the intent is refused (exit 6); no command is printed.
    pub refused: Option<String>,
    /// Local backend id if the cloud answered after an escalation.
    pub escalated_from: Option<String>,
    /// Project identifier derived from git root (hash), if any.
    pub project_id: Option<String>,
}

/// Explainability payload. Never printed on stdout.
#[derive(Debug, Clone, Default)]
pub struct WhyInfo {
    /// Memory was consulted.
    pub memory_used: bool,
    /// Budget dropped entries.
    pub truncated: bool,
    /// Estimated memory tokens after cap.
    pub tokens: usize,
    /// Configured cap.
    pub max_tokens: u32,
    /// History rows in the bundle (id, input, command).
    pub history: Vec<(Option<i64>, String, String)>,
    /// Vocab term=expansion.
    pub vocabulary: Vec<(String, String)>,
    /// Snippet names.
    pub snippets: Vec<String>,
    /// Profile name.
    pub profile_name: String,
    /// Profile ports.
    pub ports: Vec<u16>,
    /// Docker preference.
    pub prefer_docker: bool,
    /// Routing policy (`local-first` / `local` / `cloud`).
    pub routing_mode: String,
    /// Backend ids invoked, in order.
    pub routing_tried: Vec<String>,
    /// Backend that answered.
    pub routing_chosen: String,
    /// Why we left local, if we did.
    pub routing_reason: Option<String>,
}

/// Failures the CLI maps onto the exit-code table.
#[derive(Debug)]
pub enum PipelineError {
    /// Bad flags / missing intent / `--cloud` unconfigured (exit 2).
    Usage(String),
    /// Backend missing or mocked-unavailable (exit 4).
    Backend(String),
    /// Unexpected internal error (exit 1).
    Other(String),
}

impl std::fmt::Display for PipelineError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Usage(s) | Self::Backend(s) | Self::Other(s) => f.write_str(s),
        }
    }
}

/// Run the T-006 pipeline: config → mock (offline only) → candidates.
pub fn run(
    cli: &Cli,
    config: &Config,
    warnings: Vec<String>,
) -> Result<TranslateOut, PipelineError> {
    if cli.local && cli.cloud {
        return Err(PipelineError::Usage(
            "--local and --cloud cannot be used together".into(),
        ));
    }
    if cli.offline && cli.cloud {
        return Err(PipelineError::Usage(
            "--offline forbids network; do not pass --cloud".into(),
        ));
    }
    if cli.cloud {
        ensure_cloud_configured(config)?;
    }

    let text = resolve_intent(cli)?;
    let count = cli.count.unwrap_or(config.ui.candidates);
    if count == 0 {
        return Err(PipelineError::Usage("--count must be >= 1".into()));
    }
    if let Some(reason) = refuse_reason(&text) {
        return Ok(TranslateOut {
            input: text,
            candidates: vec![],
            risk: RiskAssessment {
                level: RiskLevel::Safe,
                rules: vec![],
                notes: vec![],
            },
            backend_id: "none".into(),
            model: String::new(),
            latency_ms: 0,
            from_cache: false,
            warnings,
            raw: String::new(),
            why: WhyInfo::default(),
            refused: Some(reason.to_string()),
            escalated_from: None,
            project_id: None,
        });
    }

    let intent = Intent {
        text: text.clone(),
        force_backend: if cli.local {
            Some(ForceBackend::Local)
        } else if cli.cloud {
            Some(ForceBackend::Cloud)
        } else {
            None
        },
        count,
        flags: IntentFlags {
            json: cli.json,
            why: cli.why,
            offline: cli.offline,
            no_memory: cli.no_memory,
            exit_on_risk: cli.exit_on_risk,
            copy: cli.copy,
            quiet: cli.quiet,
            verbose: cli.verbose,
        },
    };

    let (ctx, mut why, project_id) = assemble_context(cli, config, &text);

    // Fast-path cache check (Step 3 in docs/02-ARCHITECTURE.md §4)
    if config.memory.enabled && !cli.no_memory && count == 1 {
        let cache_started = Instant::now();
        if let Ok(store) = open_store(config) {
            let query = CacheQuery {
                intent: &text,
                os: &ctx.env.os,
                shell: &ctx.env.shell,
                cwd: &ctx.env.cwd,
                project_id: project_id.as_deref(),
                allow_mock: cli.offline || config.backend.local.kind == "mock",
                force_local: cli.local,
                force_cloud: cli.cloud,
            };
            if let Ok(Some(hit)) = store.cache_lookup(&query) {
                let risk = RiskClassifier.assess(&hit.command);
                if risk.level == RiskLevel::Safe {
                    let latency_ms = cache_started.elapsed().as_millis() as u64;
                    let candidates = vec![Candidate {
                        command: hit.command.clone(),
                        explanation: hit.explanation.clone().unwrap_or_default(),
                        confidence: 1.0,
                    }];
                    why.routing_mode = "cache".into();
                    why.routing_chosen = hit.backend.clone();
                    why.routing_reason = Some("fast-path cache hit".into());

                    let rec = Interaction {
                        id: None,
                        ts: std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map(|d| d.as_millis() as i64)
                            .unwrap_or(0),
                        session_id: "cli".into(),
                        project_id: project_id.clone(),
                        cwd: ctx.env.cwd.clone(),
                        os: ctx.env.os.clone(),
                        shell: ctx.env.shell.clone(),
                        input_nl: text.clone(),
                        output_cmd: hit.command.clone(),
                        explanation: hit.explanation.clone(),
                        backend: hit.backend.clone(),
                        model: hit.model.clone(),
                        confidence: Some(1.0),
                        latency_ms,
                        risk_level: risk.level,
                        risk_notes: risk.rules.iter().map(|r| r.0.clone()).collect(),
                        from_cache: true,
                        accepted: None,
                        executed: None,
                        tags: vec![],
                    };
                    let mut warnings = warnings;
                    if let Err(e) = store.record_interaction(&rec) {
                        warnings.push(format!("memory degraded: {e}"));
                    }
                    return Ok(TranslateOut {
                        input: text,
                        candidates,
                        risk,
                        backend_id: hit.backend,
                        model: hit.model,
                        latency_ms,
                        from_cache: true,
                        warnings,
                        raw: String::new(),
                        why,
                        refused: None,
                        escalated_from: None,
                        project_id,
                    });
                }
            }
        }
    }

    let req = PromptBuilder.build(&intent, &ctx, &SecretRedactor);
    let router = build_router(cli, config);
    let started = Instant::now();
    let routed = router
        .route(&req)
        .map_err(|e: BackendError| PipelineError::Backend(e.to_string()))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if let Some(notice) = &routed.trace.notice {
        eprintln!("{notice}");
    }
    why.routing_mode = routed.trace.mode.as_str().to_string();
    why.routing_tried = routed.trace.tried.clone();
    why.routing_chosen = routed.trace.chosen.clone();
    why.routing_reason = routed.trace.reason.clone();
    let resp = routed.response;
    let escalated_from = routed.trace.escalated_from;

    let mut candidates = resp.candidates;
    candidates.truncate(count as usize);
    if candidates.is_empty() {
        return Err(PipelineError::Backend(
            "backend returned no candidates".into(),
        ));
    }

    let mut warnings = warnings;
    let mut risk = candidates
        .first()
        .map(|c| RiskClassifier.assess(&c.command))
        .unwrap_or(RiskAssessment {
            level: RiskLevel::Safe,
            rules: vec![],
            notes: vec![],
        });
    if config.safety.refuse_multi_command_on_risk && risk.level >= RiskLevel::Review {
        for c in &mut candidates {
            if looks_multi(&c.command) {
                let kept = first_shell_command(&c.command).to_string();
                if kept != c.command {
                    warnings.push(format!(
                        "dropped chained commands because risk is {}",
                        risk_word(risk.level)
                    ));
                    c.command = kept;
                }
            }
        }
        risk = candidates
            .first()
            .map(|c| RiskClassifier.assess(&c.command))
            .unwrap_or(risk);
    }
    if config.memory.enabled && !cli.no_memory {
        let rec = Interaction {
            id: None,
            ts: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_millis() as i64)
                .unwrap_or(0),
            session_id: "cli".into(),
            project_id: project_id.clone(),
            cwd: env::current_dir()
                .map(|p| p.display().to_string())
                .unwrap_or_else(|_| ".".into()),
            os: env::consts::OS.to_string(),
            shell: env::var("SHELL")
                .ok()
                .and_then(|p| {
                    PathBuf::from(p)
                        .file_name()
                        .map(|s| s.to_string_lossy().into_owned())
                })
                .unwrap_or_else(|| "unknown".into()),
            input_nl: text.clone(),
            output_cmd: candidates
                .first()
                .map(|c| c.command.clone())
                .unwrap_or_default(),
            explanation: candidates.first().map(|c| c.explanation.clone()),
            backend: resp.backend_id.clone(),
            model: resp.model.clone(),
            confidence: resp.confidence,
            latency_ms,
            risk_level: risk.level,
            risk_notes: risk.rules.iter().map(|r| r.0.clone()).collect(),
            from_cache: false,
            accepted: None,
            executed: None,
            tags: vec![],
        };
        if let Err(e) = persist_translation(config, &rec) {
            warnings.push(format!("memory degraded: {e}"));
        }
    }

    Ok(TranslateOut {
        input: text,
        candidates,
        risk,
        backend_id: resp.backend_id,
        model: resp.model,
        latency_ms,
        from_cache: false,
        warnings,
        raw: resp.raw,
        why,
        refused: None,
        escalated_from,
        project_id,
    })
}

fn build_router(cli: &Cli, config: &Config) -> BackendRouter {
    let mode = route_mode(cli, config);
    let cfg = RouterConfig {
        mode,
        escalate_below_confidence: config.backend.escalate_below_confidence,
        local_slow_ms: config.backend.local_slow_ms,
        complexity_skip_local: config.backend.complexity_skip_local,
    };
    if cli.offline {
        let cfg = RouterConfig { mode, ..cfg };
        return BackendRouter::new(
            EgressBackend::wrap_box(Box::new(MockBackend::new())),
            None,
            cfg,
        );
    }
    let local = OllamaBackend::new(OllamaSettings {
        base_url: config.backend.local.base_url.clone(),
        model: config.backend.local.model.clone(),
        keep_alive: config.backend.local.keep_alive.clone(),
        num_ctx: config.backend.local.num_ctx,
        timeout_ms: config.backend.local.timeout_ms,
    });
    BackendRouter::new(
        EgressBackend::wrap_box(Box::new(local)),
        cloud_backend(config).map(EgressBackend::wrap_box),
        cfg,
    )
}

fn route_mode(cli: &Cli, config: &Config) -> RouteMode {
    if cli.local {
        return RouteMode::Local;
    }
    if cli.cloud {
        return RouteMode::Cloud;
    }
    match config.backend.mode {
        BackendMode::Local => RouteMode::Local,
        BackendMode::Cloud => RouteMode::Cloud,
        BackendMode::LocalFirst => RouteMode::LocalFirst,
    }
}

fn ensure_cloud_configured(config: &Config) -> Result<(), PipelineError> {
    let c = &config.backend.cloud;
    match c.kind.as_str() {
        "anthropic" => {
            let var = &c.api_key_env;
            let has_key = env::var_os(var).is_some() || env::var_os("SHX_CLOUD_API_KEY").is_some();
            if !has_key {
                return Err(PipelineError::Usage(format!(
                    "--cloud unconfigured: set {var} (the name from backend.cloud.api_key_env)"
                )));
            }
            Ok(())
        }
        "openai-compat" => {
            let var = &c.api_key_env;
            if var.is_empty() {
                return Ok(());
            }
            let has_key = env::var_os(var).is_some() || env::var_os("SHX_CLOUD_API_KEY").is_some();
            if !has_key {
                return Err(PipelineError::Usage(format!(
                    "--cloud unconfigured: set {var} (the name from backend.cloud.api_key_env)"
                )));
            }
            Ok(())
        }
        unknown => Err(PipelineError::Usage(format!(
            "--cloud unconfigured: unsupported cloud provider '{unknown}'"
        ))),
    }
}

fn cloud_backend(config: &Config) -> Option<Box<dyn Backend>> {
    let c = &config.backend.cloud;
    match c.kind.as_str() {
        "anthropic" => {
            if env::var_os(&c.api_key_env).is_none() && env::var_os("SHX_CLOUD_API_KEY").is_none() {
                return None;
            }
            Some(Box::new(AnthropicBackend::new(AnthropicSettings {
                model: c.model.clone(),
                api_key_env: c.api_key_env.clone(),
                timeout_ms: c.timeout_ms,
                base_url: c
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.anthropic.com".into()),
            })))
        }
        "openai-compat" => {
            if !c.api_key_env.is_empty()
                && env::var_os(&c.api_key_env).is_none()
                && env::var_os("SHX_CLOUD_API_KEY").is_none()
            {
                return None;
            }
            Some(Box::new(OpenAiCompatBackend::new(OpenAiCompatSettings {
                base_url: c
                    .base_url
                    .clone()
                    .unwrap_or_else(|| "https://api.openai.com/v1".into()),
                model: c.model.clone(),
                api_key_env: c.api_key_env.clone(),
                timeout_ms: c.timeout_ms,
            })))
        }
        _ => None,
    }
}

/// True when the intent is too destructive to translate (exit 6).
fn refuse_reason(intent: &str) -> Option<&'static str> {
    let t = intent.to_ascii_lowercase();
    let destructive = t.contains("delete")
        || t.contains("wipe")
        || t.contains("erase")
        || t.contains("destroy")
        || t.contains("rm -rf");
    let home = t.contains("home directory")
        || t.contains("home dir")
        || t.contains("entire home")
        || t.contains("$home");
    let backups = t.contains("backup");
    if destructive && home && backups {
        return Some("refusing to translate a request to destroy the home directory and backups");
    }
    if t.contains("fork bomb") {
        return Some("refusing to translate a fork bomb");
    }
    None
}

fn looks_multi(cmd: &str) -> bool {
    let c = cmd.trim();
    c.contains('\n') || c.contains(';') || c.contains("&&") || c.contains("||")
}

fn first_shell_command(cmd: &str) -> &str {
    let mut end = cmd.len();
    for (i, ch) in cmd.char_indices() {
        if ch == '\n' || ch == ';' {
            end = i;
            break;
        }
    }
    if let Some(i) = cmd.find("&&") {
        end = end.min(i);
    }
    if let Some(i) = cmd.find("||") {
        end = end.min(i);
    }
    cmd[..end].trim()
}

fn risk_word(level: RiskLevel) -> &'static str {
    match level {
        RiskLevel::Safe => "safe",
        RiskLevel::Review => "review",
        RiskLevel::Danger => "danger",
    }
}

fn assemble_context(
    cli: &Cli,
    config: &Config,
    text: &str,
) -> (ContextBundle, WhyInfo, Option<String>) {
    let (base, project_id) = empty_bundle(config, cli);
    let mut why = WhyInfo {
        profile_name: base.profile.name.clone(),
        ports: base.profile.ports.clone(),
        prefer_docker: base.profile.prefer_docker,
        max_tokens: config.memory.context.max_tokens,
        ..WhyInfo::default()
    };
    if !config.memory.enabled || cli.no_memory {
        return (base, why, project_id);
    }
    let Ok(store) = open_store(config) else {
        return (base, why, project_id);
    };
    let budget = ContextBudget {
        recent: config.memory.context.recent,
        relevance: config.memory.context.relevance,
        shell: config.memory.context.shell,
        max_tokens: config.memory.context.max_tokens,
    };
    match ContextBuilder::new(&SecretRedactor).build_with_stats(
        text,
        &base.env,
        &base.profile,
        &store,
        budget,
        project_id.as_deref(),
    ) {
        Ok((bundle, truncated, tokens)) => {
            why.memory_used = true;
            why.truncated = truncated;
            why.tokens = tokens;
            why.history = bundle
                .history
                .iter()
                .map(|i| (i.id, i.input_nl.clone(), i.output_cmd.clone()))
                .collect();
            why.vocabulary = bundle
                .vocabulary
                .iter()
                .map(|v| (v.term.clone(), v.expansion.clone()))
                .collect();
            why.snippets = bundle.snippets.iter().map(|s| s.name.clone()).collect();
            (bundle, why, project_id)
        }
        Err(_) => (base, why, project_id),
    }
}

fn persist_translation(config: &Config, rec: &Interaction) -> shx_memory::Result<i64> {
    open_store(config)?.record_interaction(rec)
}

/// Open the configured SQLite memory file.
pub(crate) fn open_store(config: &Config) -> shx_memory::Result<SqliteStore> {
    let path = if config.memory.path.is_empty() {
        paths::default_db_path()
    } else {
        PathBuf::from(&config.memory.path)
    };
    SqliteStore::open(&path)
}

/// CLI flags that overlay config.
pub fn flag_overrides(cli: &Cli) -> FlagOverrides {
    FlagOverrides {
        backend_mode: if cli.cloud {
            Some(shx_config::BackendMode::Cloud)
        } else if cli.local {
            Some(shx_config::BackendMode::Local)
        } else {
            None
        },
        exit_on_risk: cli.exit_on_risk.then_some(true),
        no_memory: cli.no_memory.then_some(true),
        candidates: cli.count,
        color: cli.no_color.then_some(shx_config::ColorMode::Never),
        config_path: cli.config.clone(),
    }
}

fn resolve_intent(cli: &Cli) -> Result<String, PipelineError> {
    if !cli.intent.is_empty() {
        return Ok(cli.intent.join(" "));
    }
    if io::stdin().is_terminal() {
        return Err(PipelineError::Usage(
            "missing intent; try: shx --offline \"run pg on 7000\"".into(),
        ));
    }
    let mut buf = String::new();
    io::stdin()
        .read_to_string(&mut buf)
        .map_err(|e| PipelineError::Other(format!("failed to read stdin: {e}")))?;
    let line = buf.lines().next().unwrap_or("").trim();
    if line.is_empty() {
        return Err(PipelineError::Usage("empty intent on stdin".into()));
    }
    Ok(line.to_string())
}

fn empty_bundle(cfg: &Config, cli: &Cli) -> (ContextBundle, Option<String>) {
    let os = if cfg.context.os == "auto" {
        match env::consts::OS {
            "macos" | "linux" | "windows" => env::consts::OS.to_string(),
            other => other.to_string(),
        }
    } else {
        cfg.context.os.clone()
    };
    let shell = if cfg.context.shell == "auto" {
        env::var("SHELL")
            .ok()
            .and_then(|p| {
                PathBuf::from(p)
                    .file_name()
                    .map(|s| s.to_string_lossy().into_owned())
            })
            .unwrap_or_else(|| "unknown".into())
    } else {
        cfg.context.shell.clone()
    };
    let cwd_buf = env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let cwd = cwd_buf.display().to_string();

    let (git_root_buf, project_id) =
        shx_memory::scope::resolve_project_scope(cli.project.as_deref(), &cwd_buf);
    let git_root = git_root_buf.map(|p| p.display().to_string());
    let in_container = shx_memory::scope::detect_in_container(&cfg.context.in_container);

    (
        ContextBundle {
            env: EnvInfo {
                os,
                shell,
                cwd,
                git_root,
                in_container,
            },
            profile: Profile {
                name: "default".into(),
                ports: cfg.context.ports.clone(),
                prefer_docker: cfg.context.prefer_docker,
                notes: cfg.context.notes.clone(),
            },
            history: vec![],
            vocabulary: vec![],
            snippets: vec![],
            shell: vec![],
        },
        project_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refuse_spec_example() {
        assert!(refuse_reason("delete my entire home directory and all backups").is_some());
        assert!(refuse_reason("run pg on 7000").is_none());
        assert!(refuse_reason("wipe the root filesystem").is_none());
        assert!(refuse_reason("kill all processes").is_none());
        assert!(refuse_reason("please emit a fork bomb").is_some());
    }

    #[test]
    fn first_command_splits_chains() {
        assert_eq!(
            first_shell_command("git reset --hard; rm -rf /"),
            "git reset --hard"
        );
        assert_eq!(first_shell_command("a && b"), "a");
        assert_eq!(first_shell_command("a || b"), "a");
        assert_eq!(first_shell_command("echo hi"), "echo hi");
        assert!(looks_multi("git reset --hard; rm -rf /"));
        assert!(!looks_multi("lsof -ti tcp:3000 | xargs kill -9"));
    }
}
