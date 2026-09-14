//! LLM backend integrations for `shx`.
//!
//! Depends inward on `shx-core` only. No async runtime (ADR-006).

#![forbid(unsafe_code)]

pub mod anthropic;
pub mod backend;
pub mod error;
pub mod http;
pub mod mock;
pub mod ollama;
pub mod openai_compat;
pub mod router;

pub use anthropic::{AnthropicBackend, AnthropicSettings};
pub use backend::{Backend, BackendError, Capabilities, CostTier, ErrorKind, Health};
pub use error::{from_bad_output, from_transport, retryable};
pub use mock::MockBackend;
pub use ollama::{OllamaBackend, OllamaSettings};
pub use openai_compat::{MetaCache, OpenAiCompatBackend, OpenAiCompatSettings, StructuredFormat};
pub use router::{BackendRouter, RouteMode, RouteTrace, RoutedTranslate, RouterConfig};
