//! Domain types and pure pipeline stages for `shx`.
//!
//! This crate is the inward layer of the workspace: it depends on no other
//! `shx-*` crate and performs no I/O.

#![forbid(unsafe_code)]

pub mod parse;
pub mod prompt;
pub mod redact;
pub mod types;

pub use parse::{ParseError, ParseErrorKind, ParsedOutput, parse_response};
pub use prompt::{PROMPT_VERSION, PromptBuilder};
pub use redact::{NoopRedactor, Redacted, Redactor};
pub use types::*;
