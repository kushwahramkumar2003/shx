//! Domain types and pure pipeline stages for `shx`.
//!
//! This crate is the inward layer of the workspace: it depends on no other
//! `shx-*` crate and performs no I/O.

#![forbid(unsafe_code)]

pub mod env;
pub mod parse;
pub mod prompt;
pub mod redact;
pub mod risk;
pub mod types;

pub use env::{compute_project_id, is_in_container, resolve_in_container};
pub use parse::{ParseError, ParseErrorKind, ParsedOutput, parse_response};
pub use prompt::{PROMPT_VERSION, PromptBuilder};
pub use redact::{NoopRedactor, Redacted, Redactor, SecretRedactor};
pub use risk::{RULES, RiskClassifier, Rule};
pub use types::*;
