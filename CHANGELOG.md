# Changelog

All notable changes to `shx` are documented here.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).
While pre-1.0, breaking changes bump the **minor** version and are called out under
`Changed`.

## [Unreleased]

### Security

- Bump `rustls` to 0.23.45 (RUSTSEC-2026-0285).

### Added

- `shx snippet save|list|show|rm` stores named command macros in local
  memory. Matching snippets enter the prompt `ContextBundle` and show up
  in `--why`. `show --copy` prints the command to stdout (clipboard is
  T-603). `shx <name>` is never auto-resolved to a snippet and is never
  executed.
- Egress redaction: every backend request (local and cloud) is run through
  `SecretRedactor` immediately before serialize. T-ROUTE-3 covers a poisoned
  fixture.
- Local-first `BackendRouter`: escalate once on timeout/error/bad-output/low
  confidence/slowness, announce on stderr, and record the trace in `--why`.
  `mode=local` never calls cloud. Translate without `--offline` uses Ollama
  (and cloud when a key is configured).
- OpenAI-compatible backend (`POST /chat/completions`) with a per-`base_url`
  `json_schema` → `json_object` capability probe cached in meta. Works with no
  API key (LM Studio). Translate still requires `--offline` until T-403.
- Anthropic backend (`POST /v1/messages`, forced single-tool `input_schema`).
  The API key is read from the env var named in config (`api_key_env`), never
  from the file. Translate still requires `--offline` until the router (T-403).
- `shx doctor --redaction-test` runs the secret-redaction corpus and reports
  per-pattern pass/fail (exit 1 if any pattern fails).
- Risk banners on stderr (yellow REVIEW, red DANGER) with *what* and *why*.
  `--exit-on-risk` exits 3 when risk ≥ Review; catastrophic intents are
  refused (exit 6, no command). Chained commands at Review/Danger keep
  only the first (`refuse_multi_command_on_risk`).
- `shx --why` prints a stderr block naming the backend, profile, budget
  truncation, and the memory entries (history, vocabulary, snippets) used.
  Combined with `--json` it still goes to stderr; stdout stays the JSON object.
- `shx teach <term> <expansion>`, `--list`, and `--forget` manage shorthand
  vocabulary (taught weight 2.0; learned terms apply only at ≥ 1.5).
- Translations pull a token-budgeted slice of prior history, vocabulary, and
  snippets into the prompt (redacted).
- `shx history` lists, shows, exports (`--json`/`--jsonl`), prunes, and
  purges stored translations (`-y --all` required to purge).
- Successful translations are stored locally (redacted). `--no-memory` skips
  the write. Memory failures warn on stderr and still print the command.
- `shx doctor` probes the configured Ollama endpoint (`GET /api/tags`) and
  exits 4 when it is unreachable; `--offline` still uses the mock.
- Ollama backend (`POST /api/chat`, `GET /api/tags`) via sync ureq+rustls.
  Translate still requires `--offline` (mock) until the pipeline is wired.
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
