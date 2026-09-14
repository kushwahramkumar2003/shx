//! Semantic validation with actionable errors.

use crate::model::Config;

/// Why a resolved config is not usable.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    /// A value was not in the accepted set (or failed a range check).
    #[error("invalid value for `{path}`: {value}; accepted: {accepted}")]
    InvalidValue {
        /// Dotted key path (`backend.mode`).
        path: String,
        /// The rejected value, Display-formatted.
        value: String,
        /// Human-readable accepted set.
        accepted: String,
    },
    /// File could not be read.
    #[error("failed to read {path}: {detail}")]
    Io {
        /// Path we tried to read.
        path: String,
        /// `io::Error` display.
        detail: String,
    },
    /// TOML syntax error.
    #[error("failed to parse {origin}: {detail}")]
    Parse {
        /// File path or `"env"` / `"flags"`.
        origin: String,
        /// Parser message.
        detail: String,
    },
}

impl ConfigError {
    /// Convenience constructor for [`ConfigError::InvalidValue`].
    pub fn invalid(
        path: impl Into<String>,
        value: impl Into<String>,
        accepted: impl Into<String>,
    ) -> Self {
        Self::InvalidValue {
            path: path.into(),
            value: value.into(),
            accepted: accepted.into(),
        }
    }
}

/// Check ranges and enum-like string fields that serde accepted as free strings.
pub fn validate(cfg: &Config) -> Result<(), ConfigError> {
    if !(0.0..=1.0).contains(&cfg.backend.escalate_below_confidence) {
        return Err(ConfigError::invalid(
            "backend.escalate_below_confidence",
            cfg.backend.escalate_below_confidence.to_string(),
            "a number in 0.0..=1.0",
        ));
    }
    match cfg.backend.local.kind.as_str() {
        "ollama" => {}
        other => {
            return Err(ConfigError::invalid("backend.local.kind", other, "ollama"));
        }
    }
    match cfg.backend.cloud.kind.as_str() {
        "anthropic" | "openai-compat" => {}
        other => {
            return Err(ConfigError::invalid(
                "backend.cloud.kind",
                other,
                "anthropic, openai-compat",
            ));
        }
    }
    match cfg.context.os.as_str() {
        "auto" | "macos" | "linux" | "windows" => {}
        other => {
            return Err(ConfigError::invalid(
                "context.os",
                other,
                "auto, macos, linux, windows",
            ));
        }
    }
    match cfg.context.in_container.as_str() {
        "auto" | "true" | "false" => {}
        other => {
            return Err(ConfigError::invalid(
                "context.in_container",
                other,
                "auto, true, false",
            ));
        }
    }
    match cfg.ui.update_check.as_str() {
        "never" => {}
        other => {
            return Err(ConfigError::invalid("ui.update_check", other, "never"));
        }
    }
    if cfg.ui.candidates == 0 {
        return Err(ConfigError::invalid(
            "ui.candidates",
            "0",
            "an integer >= 1",
        ));
    }
    if cfg.memory.retention_days == 0 {
        return Err(ConfigError::invalid(
            "memory.retention_days",
            "0",
            "an integer >= 1",
        ));
    }
    Ok(())
}
