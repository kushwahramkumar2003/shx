//! Translate pipeline for T-006: MockBackend, no memory.

use std::env;
use std::io::{self, IsTerminal, Read};
use std::path::PathBuf;
use std::time::Instant;

use shx_config::{Config, FlagOverrides};
use shx_core::{
    Candidate, ContextBundle, EnvInfo, ForceBackend, Intent, IntentFlags, NoopRedactor, Profile,
    PromptBuilder, RiskAssessment, RiskLevel,
};
use shx_llm::{Backend, BackendError, MockBackend};

use crate::Cli;

/// Successful translation ready to render.
#[derive(Debug, Clone)]
pub struct TranslateOut {
    /// Original intent text.
    pub input: String,
    /// Candidates to print (already truncated to `-n`).
    pub candidates: Vec<Candidate>,
    /// Classifier result. Always `Safe` until T-301.
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
        let var = &config.backend.cloud.api_key_env;
        if env::var_os(var).is_none() {
            return Err(PipelineError::Usage(format!(
                "--cloud unconfigured: set {var} (the name from backend.cloud.api_key_env)"
            )));
        }
        return Err(PipelineError::Backend(
            "cloud backend is not wired yet (T-401)".into(),
        ));
    }
    if !cli.offline {
        return Err(PipelineError::Backend(
            "no local backend yet (T-102); use --offline for the mock fixture".into(),
        ));
    }

    let text = resolve_intent(cli)?;
    let count = cli.count.unwrap_or(config.ui.candidates);
    if count == 0 {
        return Err(PipelineError::Usage("--count must be >= 1".into()));
    }

    let intent = Intent {
        text: text.clone(),
        force_backend: if cli.local {
            Some(ForceBackend::Local)
        } else {
            None
        },
        count,
        flags: IntentFlags {
            json: cli.json,
            why: cli.why,
            offline: cli.offline,
            no_memory: true,
            exit_on_risk: cli.exit_on_risk,
            copy: cli.copy,
            quiet: cli.quiet,
            verbose: cli.verbose,
        },
    };

    let ctx = empty_bundle(config);
    let req = PromptBuilder.build(&intent, &ctx, &NoopRedactor);
    let backend = MockBackend::new();
    let started = Instant::now();
    let resp = backend
        .translate(&req)
        .map_err(|e: BackendError| PipelineError::Backend(e.to_string()))?;
    let latency_ms = started.elapsed().as_millis() as u64;

    let mut candidates = resp.candidates;
    candidates.truncate(count as usize);
    if candidates.is_empty() {
        return Err(PipelineError::Backend(
            "backend returned no candidates".into(),
        ));
    }

    Ok(TranslateOut {
        input: text,
        candidates,
        risk: RiskAssessment {
            level: RiskLevel::Safe,
            rules: vec![],
            notes: vec![],
        },
        backend_id: resp.backend_id,
        model: resp.model,
        latency_ms,
        from_cache: false,
        warnings,
        raw: resp.raw,
    })
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
        no_memory: Some(true),
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

fn empty_bundle(cfg: &Config) -> ContextBundle {
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
    let cwd = env::current_dir()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| ".".into());
    ContextBundle {
        env: EnvInfo {
            os,
            shell,
            cwd,
            git_root: None,
            in_container: cfg.context.in_container == "true",
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
    }
}
