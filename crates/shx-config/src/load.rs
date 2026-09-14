//! Layered config merge: defaults < global < project < env < flags.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use toml::Value;

use crate::model::{BackendMode, ColorMode, Config, FlagOverrides};
use crate::validate::{ConfigError, validate};

/// Result of a load, including unknown-key warnings.
#[derive(Debug, Clone, PartialEq)]
pub struct LoadedConfig {
    /// Merged, validated config.
    pub config: Config,
    /// Unknown keys (stderr warnings). Never fatal.
    pub warnings: Vec<String>,
    /// Paths consulted, in precedence order (lowest first).
    pub sources: Vec<PathBuf>,
}

/// Inputs for a deterministic load (tests inject these).
#[derive(Debug, Clone, Default)]
pub struct LoadInputs<'a> {
    /// Global `shx.toml` contents.
    pub global_toml: Option<&'a str>,
    /// Project `.shx.toml` contents.
    pub project_toml: Option<&'a str>,
    /// `KEY=VALUE` pairs (typically `SHX_*`).
    pub env: &'a [(String, String)],
    /// CLI flag overlay.
    pub flags: FlagOverrides,
}

/// Load from real files + process env + flags.
pub fn load(flags: FlagOverrides) -> Result<LoadedConfig, ConfigError> {
    let global_path = flags
        .config_path
        .as_deref()
        .map(PathBuf::from)
        .unwrap_or_else(default_global_path);
    let project_path = discover_project_toml();

    let mut sources = Vec::new();
    let global_toml = read_optional(&global_path, &mut sources)?;
    let project_toml = match &project_path {
        Some(p) => read_optional(p, &mut sources)?,
        None => None,
    };

    let env_pairs: Vec<(String, String)> =
        env::vars().filter(|(k, _)| k.starts_with("SHX_")).collect();

    let mut loaded = load_from(LoadInputs {
        global_toml: global_toml.as_deref(),
        project_toml: project_toml.as_deref(),
        env: &env_pairs,
        flags,
    })?;
    loaded.sources = sources;
    Ok(loaded)
}

/// Layered merge used by tests and [`load`].
pub fn load_from(inputs: LoadInputs<'_>) -> Result<LoadedConfig, ConfigError> {
    let mut warnings = Vec::new();
    let mut cfg = Config::default();

    if let Some(text) = inputs.global_toml {
        apply_toml(&mut cfg, text, "global", &mut warnings)?;
    }
    if let Some(text) = inputs.project_toml {
        apply_toml(&mut cfg, text, "project", &mut warnings)?;
    }
    apply_env(&mut cfg, inputs.env, &mut warnings)?;
    apply_flags(&mut cfg, &inputs.flags);
    validate(&cfg)?;
    Ok(LoadedConfig {
        config: cfg,
        warnings,
        sources: Vec::new(),
    })
}

fn read_optional(path: &Path, sources: &mut Vec<PathBuf>) -> Result<Option<String>, ConfigError> {
    match fs::read_to_string(path) {
        Ok(s) => {
            sources.push(path.to_path_buf());
            Ok(Some(s))
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(ConfigError::Io {
            path: path.display().to_string(),
            detail: e.to_string(),
        }),
    }
}

/// Global config path for this OS.
pub fn default_global_path() -> PathBuf {
    if let Ok(xdg) = env::var("XDG_CONFIG_HOME")
        && !xdg.is_empty()
    {
        return PathBuf::from(xdg).join("shx").join("shx.toml");
    }
    #[cfg(target_os = "macos")]
    {
        if let Some(home) = env::var_os("HOME") {
            return PathBuf::from(home)
                .join("Library")
                .join("Application Support")
                .join("shx")
                .join("shx.toml");
        }
    }
    #[cfg(windows)]
    {
        if let Some(appdata) = env::var_os("APPDATA") {
            return PathBuf::from(appdata).join("shx").join("shx.toml");
        }
    }
    if let Some(home) = env::var_os("HOME") {
        return PathBuf::from(home)
            .join(".config")
            .join("shx")
            .join("shx.toml");
    }
    PathBuf::from("shx.toml")
}

/// Walk up from cwd looking for `.shx.toml` next to `.git`, else cwd.
pub fn discover_project_toml() -> Option<PathBuf> {
    let cwd = env::current_dir().ok()?;
    let mut dir = cwd.as_path();
    loop {
        let candidate = dir.join(".shx.toml");
        if candidate.is_file() {
            return Some(candidate);
        }
        if dir.join(".git").exists() {
            let at_root = dir.join(".shx.toml");
            return if at_root.is_file() {
                Some(at_root)
            } else {
                None
            };
        }
        dir = dir.parent()?;
    }
}

/// Paths in precedence order (existing or not), for `config path`.
pub fn discovered_paths() -> Vec<PathBuf> {
    let mut v = vec![default_global_path()];
    if let Some(p) = discover_project_toml() {
        v.push(p);
    }
    v
}

fn apply_toml(
    cfg: &mut Config,
    text: &str,
    origin: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let value: Value = text
        .parse()
        .map_err(|e: toml::de::Error| ConfigError::Parse {
            origin: origin.into(),
            detail: e.to_string(),
        })?;
    let table = match value {
        Value::Table(t) => t,
        _ => {
            return Err(ConfigError::Parse {
                origin: origin.into(),
                detail: "root must be a table".into(),
            });
        }
    };
    apply_table(cfg, &table, "", warnings)
}

fn apply_env(
    cfg: &mut Config,
    env: &[(String, String)],
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    let mut table = toml::map::Map::new();
    for (key, val) in env {
        let Some(rest) = key.strip_prefix("SHX_") else {
            continue;
        };
        if rest.is_empty() {
            continue;
        }
        let parts: Vec<String> = rest
            .split("__")
            .map(|p| p.to_ascii_lowercase().replace('_', "-"))
            .collect();
        // `SHX_BACKEND__MODE` → backend.mode; keep underscores inside a segment
        // as hyphen only when the config key uses hyphen (`local-first` is a
        // *value*, not a key). Restore nested keys that use underscore in the
        // spec (escalate_below_confidence) by mapping known names.
        let parts = normalize_env_parts(parts);
        insert_nested(&mut table, &parts, env_value(val));
    }
    if table.is_empty() {
        return Ok(());
    }
    apply_table(cfg, &table, "", warnings)
}

fn normalize_env_parts(parts: Vec<String>) -> Vec<String> {
    parts
        .into_iter()
        .map(|p| match p.as_str() {
            "escalate-below-confidence" => "escalate_below_confidence".into(),
            "local-slow-ms" => "local_slow_ms".into(),
            "complexity-skip-local" => "complexity_skip_local".into(),
            "keep-alive" => "keep_alive".into(),
            "num-ctx" => "num_ctx".into(),
            "timeout-ms" => "timeout_ms".into(),
            "api-key-env" => "api_key_env".into(),
            "base-url" => "base_url".into(),
            "retention-days" => "retention_days".into(),
            "ingest-shell-history" => "ingest_shell_history".into(),
            "redact-secrets" => "redact_secrets".into(),
            "max-tokens" => "max_tokens".into(),
            "warn-on-risk" => "warn_on_risk".into(),
            "exit-on-risk" => "exit_on_risk".into(),
            "refuse-multi-command-on-risk" => "refuse_multi_command_on_risk".into(),
            "in-container" => "in_container".into(),
            "prefer-docker" => "prefer_docker".into(),
            "update-check" => "update_check".into(),
            other => other.to_string(),
        })
        .collect()
}

fn env_value(raw: &str) -> Value {
    if let Ok(v) = raw.parse::<Value>() {
        return v;
    }
    Value::String(raw.to_string())
}

fn insert_nested(table: &mut toml::map::Map<String, Value>, parts: &[String], val: Value) {
    if parts.is_empty() {
        return;
    }
    if parts.len() == 1 {
        table.insert(parts[0].clone(), val);
        return;
    }
    let entry = table
        .entry(parts[0].clone())
        .or_insert(Value::Table(toml::map::Map::new()));
    if let Value::Table(child) = entry {
        insert_nested(child, &parts[1..], val);
    }
}

fn apply_flags(cfg: &mut Config, flags: &FlagOverrides) {
    if let Some(mode) = flags.backend_mode {
        cfg.backend.mode = mode;
    }
    if let Some(v) = flags.exit_on_risk {
        cfg.safety.exit_on_risk = v;
    }
    if let Some(true) = flags.no_memory {
        cfg.memory.enabled = false;
    }
    if let Some(n) = flags.candidates {
        cfg.ui.candidates = n;
    }
    if let Some(c) = flags.color {
        cfg.ui.color = c;
    }
}

fn apply_table(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match (prefix, key.as_str()) {
            ("", "backend") => {
                let t = expect_table(value, &path)?;
                apply_backend(cfg, t, &path, warnings)?;
            }
            ("", "memory") => {
                let t = expect_table(value, &path)?;
                apply_memory(cfg, t, &path, warnings)?;
            }
            ("", "safety") => {
                let t = expect_table(value, &path)?;
                apply_safety(cfg, t, &path, warnings)?;
            }
            ("", "context") => {
                let t = expect_table(value, &path)?;
                apply_context(cfg, t, &path, warnings)?;
            }
            ("", "ui") => {
                let t = expect_table(value, &path)?;
                apply_ui(cfg, t, &path, warnings)?;
            }
            ("", "snippets") => {
                let t = expect_table(value, &path)?;
                apply_snippets(cfg, t, &path, warnings)?;
            }
            ("", _) => warnings.push(format!("unknown key `{path}`")),
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_backend(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "mode" => {
                let s = expect_str(value, &path)?;
                cfg.backend.mode = BackendMode::from_str_cfg(s)
                    .ok_or_else(|| ConfigError::invalid(&path, s, BackendMode::accepted()))?;
            }
            "escalate_below_confidence" => {
                cfg.backend.escalate_below_confidence = expect_f32(value, &path)?;
            }
            "local_slow_ms" => cfg.backend.local_slow_ms = expect_u64(value, &path)?,
            "complexity_skip_local" => {
                cfg.backend.complexity_skip_local = expect_bool(value, &path)?;
            }
            "local" => {
                let t = expect_table(value, &path)?;
                apply_local(&mut cfg.backend, t, &path, warnings)?;
            }
            "cloud" => {
                let t = expect_table(value, &path)?;
                apply_cloud(&mut cfg.backend, t, &path, warnings)?;
            }
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_local(
    cfg: &mut crate::model::BackendConfig,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "kind" => cfg.local.kind = expect_str(value, &path)?.to_string(),
            "base_url" => cfg.local.base_url = expect_str(value, &path)?.to_string(),
            "model" => cfg.local.model = expect_str(value, &path)?.to_string(),
            "keep_alive" => cfg.local.keep_alive = expect_str(value, &path)?.to_string(),
            "num_ctx" => cfg.local.num_ctx = expect_u64(value, &path)? as u32,
            "timeout_ms" => cfg.local.timeout_ms = expect_u64(value, &path)?,
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_cloud(
    cfg: &mut crate::model::BackendConfig,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "kind" => cfg.cloud.kind = expect_str(value, &path)?.to_string(),
            "model" => cfg.cloud.model = expect_str(value, &path)?.to_string(),
            "api_key_env" => cfg.cloud.api_key_env = expect_str(value, &path)?.to_string(),
            "timeout_ms" => cfg.cloud.timeout_ms = expect_u64(value, &path)?,
            "base_url" => {
                cfg.cloud.base_url = match value {
                    Value::String(s) if s.is_empty() => None,
                    Value::String(s) => Some(s.clone()),
                    other => {
                        return Err(ConfigError::invalid(
                            &path,
                            format_val(other),
                            "a string URL",
                        ));
                    }
                };
            }
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_memory(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "enabled" => cfg.memory.enabled = expect_bool(value, &path)?,
            "path" => cfg.memory.path = expect_str(value, &path)?.to_string(),
            "retention_days" => cfg.memory.retention_days = expect_u64(value, &path)? as u32,
            "ingest_shell_history" => {
                cfg.memory.ingest_shell_history = expect_bool(value, &path)?;
            }
            "redact_secrets" => cfg.memory.redact_secrets = expect_bool(value, &path)?,
            "context" => {
                let t = expect_table(value, &path)?;
                for (k, v) in t {
                    let p = dotted(&path, k);
                    match k.as_str() {
                        "recent" => cfg.memory.context.recent = expect_u64(v, &p)? as u32,
                        "relevance" => cfg.memory.context.relevance = expect_u64(v, &p)? as u32,
                        "shell" => cfg.memory.context.shell = expect_u64(v, &p)? as u32,
                        "max_tokens" => cfg.memory.context.max_tokens = expect_u64(v, &p)? as u32,
                        _ => warnings.push(format!("unknown key `{p}`")),
                    }
                }
            }
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_safety(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "warn_on_risk" => cfg.safety.warn_on_risk = expect_bool(value, &path)?,
            "exit_on_risk" => cfg.safety.exit_on_risk = expect_bool(value, &path)?,
            "refuse_multi_command_on_risk" => {
                cfg.safety.refuse_multi_command_on_risk = expect_bool(value, &path)?;
            }
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_context(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "os" => cfg.context.os = expect_str(value, &path)?.to_string(),
            "shell" => cfg.context.shell = expect_str(value, &path)?.to_string(),
            "in_container" => {
                cfg.context.in_container = match value {
                    Value::Boolean(b) => b.to_string(),
                    Value::String(s) => s.clone(),
                    other => {
                        return Err(ConfigError::invalid(
                            &path,
                            format_val(other),
                            "auto, true, false",
                        ));
                    }
                };
            }
            "prefer_docker" => cfg.context.prefer_docker = expect_bool(value, &path)?,
            "ports" => {
                let arr = value.as_array().ok_or_else(|| {
                    ConfigError::invalid(&path, format_val(value), "an array of port numbers")
                })?;
                let mut ports = Vec::new();
                for (i, item) in arr.iter().enumerate() {
                    ports.push(expect_u64(item, &format!("{path}[{i}]"))? as u16);
                }
                cfg.context.ports = ports;
            }
            "notes" => cfg.context.notes = expect_str(value, &path)?.to_string(),
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_ui(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "color" => {
                let s = expect_str(value, &path)?;
                cfg.ui.color = ColorMode::from_str_cfg(s)
                    .ok_or_else(|| ConfigError::invalid(&path, s, ColorMode::accepted()))?;
            }
            "timing" => cfg.ui.timing = expect_bool(value, &path)?,
            "update_check" => cfg.ui.update_check = expect_str(value, &path)?.to_string(),
            "candidates" => cfg.ui.candidates = expect_u64(value, &path)? as u8,
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn apply_snippets(
    cfg: &mut Config,
    table: &toml::map::Map<String, Value>,
    prefix: &str,
    warnings: &mut Vec<String>,
) -> Result<(), ConfigError> {
    for (key, value) in table {
        let path = dotted(prefix, key);
        match key.as_str() {
            "remote" => {
                cfg.snippets.remote = match value {
                    Value::String(s) if s.is_empty() => None,
                    Value::String(s) => Some(s.clone()),
                    other => {
                        return Err(ConfigError::invalid(
                            &path,
                            format_val(other),
                            "a string URL or empty",
                        ));
                    }
                };
            }
            _ => warnings.push(format!("unknown key `{path}`")),
        }
    }
    Ok(())
}

fn dotted(prefix: &str, key: &str) -> String {
    if prefix.is_empty() {
        key.to_string()
    } else {
        format!("{prefix}.{key}")
    }
}

fn expect_table<'a>(
    value: &'a Value,
    path: &str,
) -> Result<&'a toml::map::Map<String, Value>, ConfigError> {
    value
        .as_table()
        .ok_or_else(|| ConfigError::invalid(path, format_val(value), "a table"))
}

fn expect_str<'a>(value: &'a Value, path: &str) -> Result<&'a str, ConfigError> {
    value
        .as_str()
        .ok_or_else(|| ConfigError::invalid(path, format_val(value), "a string"))
}

fn expect_bool(value: &Value, path: &str) -> Result<bool, ConfigError> {
    value
        .as_bool()
        .ok_or_else(|| ConfigError::invalid(path, format_val(value), "true, false"))
}

fn expect_u64(value: &Value, path: &str) -> Result<u64, ConfigError> {
    match value {
        Value::Integer(i) if *i >= 0 => Ok(*i as u64),
        other => Err(ConfigError::invalid(
            path,
            format_val(other),
            "a non-negative integer",
        )),
    }
}

fn expect_f32(value: &Value, path: &str) -> Result<f32, ConfigError> {
    match value {
        Value::Float(f) => Ok(*f as f32),
        Value::Integer(i) => Ok(*i as f32),
        other => Err(ConfigError::invalid(path, format_val(other), "a number")),
    }
}

fn format_val(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::BackendMode;

    fn env_pair(k: &str, v: &str) -> (String, String) {
        (k.to_string(), v.to_string())
    }

    #[test]
    fn fixture_parses() {
        let text = include_str!("../tests/fixtures/shx.toml");
        let loaded = load_from(LoadInputs {
            global_toml: Some(text),
            ..LoadInputs::default()
        })
        .expect("fixture");
        assert_eq!(loaded.config.backend.mode, BackendMode::LocalFirst);
        assert_eq!(loaded.config.backend.local.model, "qwen3:14b");
        assert_eq!(loaded.config.context.ports, vec![3000, 5432, 7000]);
        assert!(loaded.warnings.is_empty());
    }

    #[test]
    fn precedence_defaults_global_project_env_flags() {
        let global = r#"
            [backend]
            mode = "local"
            [backend.local]
            model = "from-global"
        "#;
        let project = r#"
            [backend.local]
            model = "from-project"
        "#;
        let env = [env_pair("SHX_BACKEND__LOCAL__MODEL", "from-env")];
        let flags = FlagOverrides {
            backend_mode: Some(BackendMode::Cloud),
            ..FlagOverrides::default()
        };

        let loaded = load_from(LoadInputs {
            global_toml: Some(global),
            project_toml: Some(project),
            env: &env,
            flags,
        })
        .expect("merge");
        // flags win on mode; env wins on model over project/global
        assert_eq!(loaded.config.backend.mode, BackendMode::Cloud);
        assert_eq!(loaded.config.backend.local.model, "from-env");
    }

    #[test]
    fn env_nested_double_underscore() {
        let env = [env_pair("SHX_BACKEND__MODE", "cloud")];
        let loaded = load_from(LoadInputs {
            env: &env,
            ..LoadInputs::default()
        })
        .expect("env");
        assert_eq!(loaded.config.backend.mode, BackendMode::Cloud);
        // unspecified keys stay at defaults
        assert_eq!(loaded.config.backend.local.model, "qwen3:14b");
    }

    #[test]
    fn unknown_key_is_warning_not_error() {
        let toml = r#"
            extra_thing = 1
            [backend]
            mystery = true
            mode = "local"
        "#;
        let loaded = load_from(LoadInputs {
            global_toml: Some(toml),
            ..LoadInputs::default()
        })
        .expect("unknown keys");
        assert_eq!(loaded.config.backend.mode, BackendMode::Local);
        assert!(
            loaded.warnings.iter().any(|w| w.contains("extra_thing")),
            "{:?}",
            loaded.warnings
        );
        assert!(
            loaded
                .warnings
                .iter()
                .any(|w| w.contains("backend.mystery")),
            "{:?}",
            loaded.warnings
        );
    }

    #[test]
    fn invalid_value_names_path_and_accepted_set() {
        let toml = r#"
            [backend]
            mode = "spaceship"
        "#;
        let err = load_from(LoadInputs {
            global_toml: Some(toml),
            ..LoadInputs::default()
        })
        .unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("backend.mode"), "{msg}");
        assert!(msg.contains("spaceship"), "{msg}");
        assert!(msg.contains("local-first"), "{msg}");
    }

    #[test]
    fn flags_override_env() {
        let env = [env_pair("SHX_BACKEND__MODE", "cloud")];
        let flags = FlagOverrides {
            backend_mode: Some(BackendMode::Local),
            exit_on_risk: Some(true),
            no_memory: Some(true),
            ..FlagOverrides::default()
        };
        let loaded = load_from(LoadInputs {
            env: &env,
            flags,
            ..LoadInputs::default()
        })
        .expect("flags");
        assert_eq!(loaded.config.backend.mode, BackendMode::Local);
        assert!(loaded.config.safety.exit_on_risk);
        assert!(!loaded.config.memory.enabled);
    }
}
