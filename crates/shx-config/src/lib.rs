//! Layered configuration for `shx`.
//!
//! Depends inward on `shx-core` only.

#![forbid(unsafe_code)]

pub mod load;
pub mod model;
pub mod validate;

pub use load::{LoadInputs, LoadedConfig, discovered_paths, load, load_from};
pub use model::{
    BackendConfig, BackendMode, CloudBackendConfig, ColorMode, Config, ContextConfig,
    FlagOverrides, LocalBackendConfig, MemoryConfig, MemoryContextBudget, SafetyConfig,
    SnippetsConfig, UiConfig,
};
pub use validate::{ConfigError, validate};
