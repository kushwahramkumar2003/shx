# Changelog

All notable changes to `shx` are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While pre-1.0, breaking changes bump the **minor** version and are called out under
`Changed`.

## [Unreleased]

### Added

- `shx doctor --json` reports config parse, a stub DB path, mock backend
  health, and the effective config. `shx config init` writes a commented
  default; `shx config path` prints discovered paths.
- `shx --offline "<intent>"` prints a fixture command to stdout (mock backend);
  explanations and warnings go to stderr. `--json` emits the versioned object
  from the CLI spec.
- Workspace scaffold: Cargo workspace (`shx-core`, `shx-llm`, `shx-memory`,
  `shx-config`, `shx-cli`), pinned Rust 1.95.0, dual MIT OR Apache-2.0 license,
  and `cargo xtask ci` (the 9-step local gate).
- `shx --version` prints the package version.
- Project documentation set: vision, features, architecture, memory, safety,
  CLI spec, backends, ADRs, testing, roadmap.
- Agent guild (roles, codeship cycle, DoR/DoD, handoff) and the task board with a
  machine-readable ledger.

<!--
Entry style:
### Added / Changed / Fixed / Removed / Security
- `shx <thing>` now <behavior>. (#issue)
User-visible changes only. Internal refactors don't need a line.
-->

[Unreleased]: https://github.com/<org>/shx/compare/v0.0.0...HEAD
