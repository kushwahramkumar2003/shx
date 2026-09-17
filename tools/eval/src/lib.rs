//! `shx-eval`: deterministic T6 scoring harness (docs/08-TESTING.md §7).
//!
//! Scores `tools/eval/fixtures/translate.json` through a chosen backend
//! (offline `MockBackend` by default, live Ollama with `--live`) and reports
//! exact / regex / acceptable rates, risk-misclassification count, latency
//! percentiles, token usage, and cache-hit rate.
//!
//! In-process only: this crate translates intents and inspects the returned
//! commands. It never executes a command (ADR-002) and never spawns a
//! subprocess (T-SAFE-3).

#![forbid(unsafe_code)]

pub mod run;
pub mod scoring;

use std::path::PathBuf;

/// Fatal harness failures (exit 1/2). Per-fixture backend errors are *not*
/// fatal: they are recorded in the report and counted.
#[derive(Debug, thiserror::Error)]
pub enum EvalError {
    /// Fixture file could not be read.
    #[error("read {path}: {source}")]
    Read {
        /// Fixture path as given.
        path: PathBuf,
        /// Underlying IO error.
        source: std::io::Error,
    },
    /// Fixture file is not valid JSON.
    #[error("parse {path}: {source}")]
    Parse {
        /// Fixture path as given.
        path: PathBuf,
        /// Underlying JSON error.
        source: serde_json::Error,
    },
    /// Fixture set fails schema validation.
    #[error("{0}")]
    Schema(String),
    /// `--live` was requested but no usable Ollama is reachable.
    #[error("{0}")]
    LiveUnavailable(String),
}
