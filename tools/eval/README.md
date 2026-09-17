# eval — T6 scoring harness (T-506)

Scores [`fixtures/translate.json`](fixtures/translate.json) through a chosen
backend and prints a metrics table (the same table feeds the README
benchmarks). In-process only: it translates intents, never executes a
command (ADR-002), never spawns a subprocess (T-SAFE-3), never touches the
network unless `--live` is passed.

```sh
# Offline smoke (mock backend, deterministic, no network):
cargo run -p shx-eval -- --fixtures tools/eval/fixtures/translate.json

# Machine-readable aggregate:
cargo run -p shx-eval -- --fixtures tools/eval/fixtures/translate.json --json

# Live quality run against Ollama (needs the configured model):
SHX_LIVE_MODEL=qwen3:14b cargo run -p shx-eval -- --live \
  --fixtures tools/eval/fixtures/translate.json

# Fail when the exact-match rate regresses below a floor:
cargo run -p shx-eval -- --min-exact 0.5
```

Exit codes: `0` the gate passed, `1` an eval failure (quality gate, case
errors, or live backend unavailable), `2` usage or fixture errors. The table
(or `--json` object) goes to stdout; diagnostics go to stderr.

## How one run works

Each fixture intent is served up to `--passes` times (default 3) through one
shared in-memory store, mirroring `shx-cli/src/pipeline.rs` (prompt →
backend → risk → record):

1. **Pass 1 scores quality, risk, errors, and cold latency.** The prompt is
   built with the real `PromptBuilder`, translated by the backend, and
   assessed with the real `RiskClassifier`.
2. **Later passes score the fast-path cache.** Repeat serves hit the cache
   once an interaction is eligible (Safe + repeated 2+ times), so with the
   default 3 passes the steady-state hit rate is measured on passes 2–3.
3. Latency covers every serve; tokens sum every translate call.

## Metrics

- `exact` — trimmed command equals an alternative exactly.
- `regex` — command matches a `*` glob alternative (a pattern without `*`
  scores on equality or substring, the same contract as the T5 harness).
- `acceptable` — same leading program as the canonical (first) alternative,
  e.g. right tool but different flags. The three tiers are independent
  checks, each reported as `count/n`.
- `risk_misclassified` — produced command is `Danger` while the reference is
  not (a Safe fallback for a `Danger` reference is a backend-coverage
  artifact, not a classifier error, so it never counts).
- `errors` — backend failures or empty commands on pass 1 (listed as
  `error_ids`).
- `latency_p50_ms` / `latency_p95_ms` — nearest-rank percentiles over serves.
- `tokens_total` — summed prompt + completion tokens (`0` for the mock).
- `cache_hit_rate` — hits / lookups on repeat passes (`0/0` with one pass).

## Reading the offline table

The mock returns scripted answers for four intents and a `true # mock:
<intent>` fallback otherwise, so offline quality rates are near zero by
construction — that run validates the harness and the pipeline wiring
deterministically, it does not measure model quality (per
[08-TESTING.md](../../docs/08-TESTING.md) §8, model quality needs a real
model). One substring hit (`git-status`, whose intent echoes the pattern) is
expected. Use `--live` for real quality numbers and `--min-exact` to gate
regressions: a prompt change that drops the exact-match rate fails the run
in one command.

## Fixture schema

```jsonc
{
  "id": "git-status",                       // optional (defaults to case-N; must be unique)
  "intent": "show current git status",      // required, non-empty
  "env": { "os": "macos", "shell": "zsh", "cwd": "/tmp" },       // optional
  "profile": { "name": "default", "ports": [], "prefer_docker": false, "notes": "" }, // optional
  "expected_command": "git status",         // string or array of `*` globs (required, non-empty)
  // alias also accepted: "expected_command_regex_or_set"
}
```

## CI

`.github/workflows/eval.yml` runs on prompt/backend paths
(`shx-core/src/prompt.rs`, `shx-core/src/parse.rs`, `crates/shx-llm/**`,
`tools/eval/**`): harness self-tests plus the offline run with
`--min-exact 0`, publishing the table to the job summary.
