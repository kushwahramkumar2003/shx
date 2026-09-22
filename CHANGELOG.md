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

- Ollama chat streams internally and leaves model thinking on. `[backend.local]
  timeout_ms` is the idle gap between chunks, so a slow or thinking model is
  not cut off at 8 seconds while it is still writing. The reasoning trace is
  not printed. Stdout is only the parsed command. `num_predict` adds 2048
  tokens of headroom so the think trace does not consume the whole budget
  before the JSON command. If `message.content` is empty, the command is
  taken from `message.thinking`.
- Release packaging (cargo-dist 0.32, `dist-workspace.toml`): tag-triggered
  `.github/workflows/release.yml` builds 5 targets
  (x86_64/aarch64 × macOS/Linux-gnu + x86_64 Windows) with shell/powershell
  installers and a Homebrew formula; every archive bundles the binary, man
  page, completions, shell wrappers, licenses, and README. Scoop and winget
  ship as versioned templates (`packaging/scoop`, `packaging/winget`) filled
  with the real version/hash and uploaded by
  `packaging-manifests.yml` on each published release. Size gate (25 MB)
  enforced by `cargo xtask ci` (binary is ~5 MB). Unreleased URLs use the
  `<org>` placeholder (T-607, unblocks T-608).
- Opt-in shell wrappers (`contrib/shell/shx.zsh`, `contrib/shell/shx.bash`,
  ≤30 lines each): type a line, press `Ctrl-G`, and the `shx --exit-on-risk`
  result fills the edit buffer in place. Exit 3/4 (or anything else) leaves
  the line alone with a message; empty lines are no-ops. Never `eval` —
  buffer assignment only. Install lines in the files and in the CLI spec §8
  (T-606).
- `shx import-history --shell zsh|bash|fish [--file <path>] [--limit N]
  [--dry-run]`: opt-in ingest into `Scope::Shell` (extended + plain zsh with
  backslash-continuation unfolding, bash `HISTTIMEFORMAT` timestamps,
  fish `cmd`/`when` stanzas). Redacts every line before insert, prints
  `imported N, redacted M, skipped K malformed` to stderr (stdout stays
  empty); `--dry-run` parses and counts without opening the database.
  `--shell` defaults to `$SHELL`; `--limit` keeps the most recent N (T-605).
- `shx completion <shell>` (bash/zsh/fish/powershell/elvish) and `shx man`:
  both generated from the clap definition so they cannot drift.
  Pre-generated snapshots live at `contrib/completions/shx.*` and `man/shx.1`
  (drift-tested byte-for-byte), ready for release archives to ship (T-607).
- `-n/--count` candidates and `--copy`: `-n 3` prints one command per stdout
  line (ranked best first; fewer lines when the backend returns fewer).
  `--copy` also copies the top-ranked candidate to the system clipboard via
  arboard (optional `clipboard` feature, on by default) — in translate,
  `--explain`, `-i`, and `snippet show` modes. Without the feature, or with
  no display server, it degrades to a stderr warning and exit 0; stdout is
  never affected. The mock backend scripts the three spec §2 postgres
  alternatives for `-n` coverage (T-603, unblocks T-606).
- `shx -i` refine sessions: the initial intent comes from argv and each
  follow-up line from stdin; every turn runs the same pipeline and records
  under one shared `session_id`, so later turns see earlier ones as context.
  A blank line or EOF ends the session (last turn's exit code); turn errors
  exit 2/1/4 without extra writes, so the final command is recorded exactly
  once. Ctrl-C keeps its default disposition (immediate, no cleanup needed).
  Still print-only: one command per turn on stdout (T-602).
- `shx --explain "<command>"` reverse mode: sends the command to the
  configured backend (local-first routing, `--local`/`--cloud`/`--offline`
  honored) and prints the returned prose plus the risk banner to stderr.
  stdout stays empty except with `--json` (versioned object with
  `commands[0].command` set to the input verbatim). Never refuses, never
  reads or writes memory, never executes (T-601).
- T6 eval harness (`cargo run -p shx-eval`): scores the 20 intents in
  `tools/eval/fixtures/translate.json` through mock (offline, deterministic)
  or live Ollama (`--live`) backends, reporting exact/regex/acceptable
  rates, risk-misclassification count, p50/p95 latency, token usage, and
  cache-hit rate as a table or `--json`. `--min-exact` gates regressions;
  `.github/workflows/eval.yml` runs it on prompt/backend paths; the offline
  baseline table is in the README benchmarks section (T-506).
- `shx feedback <id> good|bad [--note ...] [--executed] [--accepted]` records
  acceptance, execution, and implicit vocabulary learning: `good` bumps every
  retriever token in the intent (`+0.5`, capped at `3.0`, creating `Learned`
  rows that apply at `>= 1.5`); `bad` lowers matching rows (`-0.5`, floored
  at `0.0`) without ever creating rows. `--executed` sets `executed=1`,
  `--accepted` forces `accepted=1`, notes are redacted before store, and
  prune-time decay (`*0.98` per 30 idle days, `FakeClock`-deterministic) is
  now boundary-consistent between the SQLite and in-memory stores (T-503).
- Fast-path cache: lookup keyed by normalized intent and context fingerprint
  `(normalized_intent, os, shell, cwd, project_id)` (< 5 ms latency). Eligible
  commands require Safe risk level and explicit acceptance (`accepted = 1`) or
  repetition (2+ times); danger results are never cached. Cache hits surface
  `from_cache: true` in `--json` and `--why` (T-502, T-MEM-4).

- Project scoping and container detection: git root discovery computes deterministic
  `project_id` hashes, enabling disjoint project-scoped recall in memory; `in_container`
  detects container runtimes via filesystem markers, cgroups, and environment;
  `--project` overrides project scoping (T-501).
- `--local` and `--cloud` flags force single-invocation backend routing. `--cloud`
  without a configured key exits 2 (usage error) and never silently falls back to
  local; `--local` forces the local backend and never escalates to cloud (T-405).

- Context builder step 2 now ranks `Tool` history with BM25 (deterministic,
  no new deps). Offline ranking-delta tests record the keyword→BM25 lift;
  T-506 will wire the same fixture into the eval harness.
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
