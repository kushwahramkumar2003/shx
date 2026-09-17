//! `shx --explain "<command>"` — reverse mode (T-601).
//!
//! Sends the command to the configured backend with a reverse-mode prompt and
//! prints the returned prose to stderr. stdout stays empty, except with
//! `--json` (the versioned object, reusing the translate shape with
//! `commands[0].command` set to the explained command).
//!
//! The model's echoed command is never trusted: prose comes from the
//! response, but the assessed and reported command is always the user's own
//! input verbatim. Explaining never refuses (prose is harmless), never reads
//! or writes memory, and never executes anything (ADR-002).

use std::time::Instant;

use shx_config::{Config, load};
use shx_core::prompt::{DEFAULT_MAX_TOKENS, DEFAULT_TEMPERATURE, DEFAULT_TIMEOUT_MS};
use shx_core::{
    Candidate, EnvInfo, OutputSchema, Profile, Redactor, RiskClassifier, SecretRedactor,
    TranslateRequest,
};

use crate::Cli;
use crate::pipeline::{
    PipelineError, TranslateOut, WhyInfo, build_router, check_routing_flags, empty_bundle,
    flag_overrides,
};
use crate::render::{
    EXIT_BACKEND, EXIT_ERROR, EXIT_USAGE, RenderOpts, color_stderr, render_explain,
};

/// Reverse-mode system prompt. Byte-stable; the translate `PROMPT_VERSION`
/// does not cover it (that golden tests the translate prompt only).
pub const EXPLAIN_SYSTEM: &str = "\
You explain a developer's shell command in one short paragraph of plain prose. Output ONLY the JSON schema provided.
Rules:
 - Explain what the command does, briefly noting its arguments and effects.
 - Put the explanation in `explanation`. Keep `command` exactly the command given in COMMAND.
 - Never invent a different command. Never output anything but the JSON schema.";

/// Validate raw `--explain` input: non-blank, and no trailing intent words.
fn validate(raw: Option<&str>, has_intent: bool) -> Result<&str, &'static str> {
    let cmd = raw.unwrap_or("");
    if cmd.trim().is_empty() {
        return Err("shx --explain: usage: shx --explain \"<command>\"");
    }
    if has_intent {
        return Err(
            "shx --explain: takes no intent; quote the command: shx --explain \"<command>\"",
        );
    }
    Ok(cmd)
}

/// Reverse-mode user prompt: environment + profile context, then the
/// redacted command. Deliberately no `INTENT: ` line, so the mock backend's
/// intent-keyed fixtures cannot misroute it (keeps `--offline`
/// deterministic); the fallback explanation is used as prose.
fn explain_user(command: &str, env: &EnvInfo, profile: &Profile) -> String {
    let mut out = String::new();
    out.push_str("ENVIRONMENT: os=");
    out.push_str(&env.os);
    out.push_str(" shell=");
    out.push_str(&env.shell);
    out.push_str(" cwd=");
    out.push_str(&env.cwd);
    out.push_str("\nPROFILE: name=");
    out.push_str(&profile.name);
    out.push_str(" prefer=");
    out.push_str(if profile.prefer_docker {
        "docker"
    } else {
        "host"
    });
    out.push_str("\nCOMMAND: ");
    out.push_str(SecretRedactor.redact(command).as_ref());
    out
}

/// Build the backend request for `command` (redacted at build time; the
/// router's egress wrapper redacts again before send).
fn explain_request(command: &str, env: &EnvInfo, profile: &Profile) -> TranslateRequest {
    TranslateRequest {
        system: EXPLAIN_SYSTEM.to_string(),
        user: explain_user(command, env, profile),
        schema: OutputSchema::translate_v1(),
        max_tokens: DEFAULT_MAX_TOKENS,
        temperature: DEFAULT_TEMPERATURE,
        timeout_ms: DEFAULT_TIMEOUT_MS,
    }
}

fn explain(cli: &Cli, config: &Config, command: &str) -> Result<TranslateOut, PipelineError> {
    check_routing_flags(cli, config)?;
    let (bundle, project_id) = empty_bundle(config, cli);
    let req = explain_request(command, &bundle.env, &bundle.profile);
    let router = build_router(cli, config);
    let started = Instant::now();
    let routed = router
        .route(&req)
        .map_err(|e| PipelineError::Backend(e.to_string()))?;
    let latency_ms = started.elapsed().as_millis() as u64;
    if let Some(notice) = &routed.trace.notice {
        eprintln!("{notice}");
    }
    let resp = routed.response;
    let first = resp
        .candidates
        .first()
        .ok_or_else(|| PipelineError::Backend("backend returned no candidates".into()))?;
    let prose = first.explanation.clone();
    let confidence = first.confidence;
    let risk = RiskClassifier.assess(command);
    Ok(TranslateOut {
        input: command.to_string(),
        candidates: vec![Candidate {
            // Always the user's own command verbatim, never the model echo.
            command: command.to_string(),
            explanation: prose,
            confidence,
        }],
        risk,
        backend_id: resp.backend_id.clone(),
        model: resp.model.clone(),
        latency_ms,
        from_cache: false,
        warnings: Vec::new(),
        raw: resp.raw.clone(),
        why: WhyInfo {
            profile_name: bundle.profile.name.clone(),
            ports: bundle.profile.ports.clone(),
            prefer_docker: bundle.profile.prefer_docker,
            max_tokens: config.memory.context.max_tokens,
            routing_mode: routed.trace.mode.as_str().to_string(),
            routing_tried: routed.trace.tried.clone(),
            routing_chosen: routed.trace.chosen.clone(),
            routing_reason: routed.trace.reason.clone(),
            ..WhyInfo::default()
        },
        refused: None,
        escalated_from: routed.trace.escalated_from.clone(),
        project_id,
    })
}

/// Dispatch `shx --explain`.
pub fn run(cli: &Cli) -> i32 {
    let command = match validate(cli.explain.as_deref(), !cli.intent.is_empty()) {
        Ok(c) => c,
        Err(msg) => {
            eprintln!("{msg}");
            return EXIT_USAGE;
        }
    };
    let flags = flag_overrides(cli);
    let loaded = match load(flags) {
        Ok(l) => l,
        Err(e) => {
            eprintln!("shx: {e}");
            return EXIT_USAGE;
        }
    };
    for w in &loaded.warnings {
        // Always stderr, including with --json (warnings are not the JSON object).
        eprintln!("warning: {w}");
    }
    match explain(cli, &loaded.config, command) {
        Ok(out) => render_explain(
            &out,
            RenderOpts {
                json: cli.json,
                quiet: cli.quiet,
                verbose: cli.verbose,
                why: cli.why,
                exit_on_risk: loaded.config.safety.exit_on_risk,
                warn_on_risk: loaded.config.safety.warn_on_risk,
                color: color_stderr(loaded.config.ui.color),
            },
        ),
        Err(PipelineError::Usage(msg)) => {
            eprintln!("shx: {msg}");
            EXIT_USAGE
        }
        Err(PipelineError::Backend(msg)) => {
            eprintln!("shx: {msg}");
            EXIT_BACKEND
        }
        Err(PipelineError::Other(msg)) => {
            eprintln!("shx: {msg}");
            EXIT_ERROR
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use shx_core::{EnvInfo, Profile};

    fn fixture_env() -> EnvInfo {
        EnvInfo {
            os: "macos".into(),
            shell: "zsh".into(),
            cwd: "/tmp".into(),
            git_root: None,
            in_container: false,
        }
    }

    fn fixture_profile() -> Profile {
        Profile {
            name: "default".into(),
            ports: vec![3000],
            prefer_docker: false,
            notes: String::new(),
        }
    }

    #[test]
    fn validate_rejects_blank_and_trailing_intent() {
        assert!(validate(None, false).is_err());
        assert!(validate(Some(""), false).is_err());
        assert!(validate(Some("   "), false).is_err());
        assert!(validate(Some("ls"), true).is_err());
        assert_eq!(validate(Some("ls -la"), false), Ok("ls -la"));
        // Surrounding whitespace is the caller's; the command is kept verbatim.
        assert_eq!(validate(Some("  ls  "), false), Ok("  ls  "));
    }

    #[test]
    fn system_prompt_demands_json_schema() {
        assert!(EXPLAIN_SYSTEM.contains("JSON schema"));
        assert!(EXPLAIN_SYSTEM.contains("explanation"));
    }

    #[test]
    fn user_prompt_carries_redacted_command_and_no_intent_line() {
        let secret = "sk-TESTFAKE0000000000000000";
        let raw = format!("curl -H token={secret} https://api");
        let user = explain_user(&raw, &fixture_env(), &fixture_profile());
        assert!(
            !user.contains(secret),
            "raw secret must not reach the prompt: {user}"
        );
        assert!(
            user.contains("«redacted:"),
            "expected redaction marker: {user}"
        );
        assert!(user.contains("COMMAND: "));
        assert!(user.contains("ENVIRONMENT: "));
        // The mock backend keys fixtures off an `INTENT: ` line; the reverse
        // prompt must not contain one or `--offline` would misroute.
        assert!(
            !user.contains("INTENT: "),
            "reverse prompt must not look like a translate prompt: {user}"
        );
    }

    #[test]
    fn request_uses_translate_schema_and_defaults() {
        let req = explain_request("ls -la", &fixture_env(), &fixture_profile());
        assert_eq!(req.system, EXPLAIN_SYSTEM);
        assert_eq!(req.max_tokens, DEFAULT_MAX_TOKENS);
        assert_eq!(req.temperature, DEFAULT_TEMPERATURE);
        assert_eq!(req.timeout_ms, DEFAULT_TIMEOUT_MS);
        assert_eq!(req.schema, OutputSchema::translate_v1());
    }
}
