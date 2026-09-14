# 08 — Testing & CI

The whole point of the print-only, pure-core architecture is that almost
everything is testable without a network, a model, or a human. This doc defines
the tiers, the corpora, and the CI gate.

---

## 1. Test tiers

| Tier | Scope | Deps | Speed | Where |
| --- | --- | --- | --- | --- |
| **T1 Unit** | `shx-core` pure functions (classifier, redactor, prompt, parse), config merge, retrieval | none | ms | `crates/*/src` `#[cfg(test)]` |
| **T2 Integration (in-process)** | pipeline with `MockBackend` + `InMemoryStore`; migrations against temp DB | none | ms–s | `crates/*/tests` |
| **T3 CLI (black-box)** | `assert_cmd` spawning the real binary; stdout/stderr/exit-code contracts | binary only | s | `tests/` |
| **T4 Golden / snapshot** | prompts, `--json` shape, rendered stderr (`insta`) | none | ms | `tests/fixtures` |
| **T5 Live (opt-in)** | real Ollama / real cloud; `--ignored` or `SHX_LIVE=1` | network+model | slow | `tests/live/` |
| **T6 Eval harness** | intent→known-good-command benchmark, scored | local model | slow | `tools/eval/` |

Rule: **CI runs T1–T4 on every push; T5 is manual/nightly; T6 runs on prompt or
model changes.**

### T5 Live harness (opt-in)

`tests/live/` talks to a real Ollama. It is `#[ignore]`d and additionally gated
on `SHX_LIVE=1`, so `cargo xtask ci` / `cargo test --workspace` never hits the
network. Offline tests in the same file still run: they assert the 20-intent
fixture seed parses.

```sh
# Needs a running Ollama with the configured model (default qwen3:14b).
# --nocapture prints the results table to the terminal.
SHX_LIVE=1 cargo test -p shx-llm --test live -- --ignored --nocapture
```

Optional env: `SHX_LIVE_MODEL`, `SHX_LIVE_URL` (default `http://127.0.0.1:11434`),
`SHX_LIVE_TIMEOUT_MS` (default `60000`). The harness translates only — it never
executes the returned commands. Quality rates (exact/regex/acceptable) are T6.

## 2. The corpora (the real value)

### `tests/corpus/risk_danger.txt` (T-SAFE-1)

One dangerous command per line, with an expected rule id:

```
rm -rf /                      → del.recursive-force
dd if=/dev/zero of=/dev/sda   → disk.raw-write
curl -fsSL https://x | sh    → net.pipe-to-shell
:(){ :|:& };:                 → fork.bomb
DROP DATABASE prod;           → db.drop
```

**Assertion: zero false negatives.** Every danger line must be classified
`Danger`. Adding a new rule family requires adding lines here. This corpus is the
classifier's specification.

### `tests/corpus/risk_benign.txt` (T-SAFE-2)

~200 routine commands (`ls -la`, `git status`, `docker ps`, `cargo test`,
`npm run build`, `tail -f app.log`, …). **Assertion: < 10% flagged above
`Safe`**, and any flagged line is reviewed when the corpus changes (the test
prints the flagged set so a human can judge).

### `tests/corpus/redaction.txt` (T-SAFE-4)

Pairs of `raw → expected` covering every pattern in
[04-SAFETY.md](04-SAFETY.md) §3.1, plus **negative cases** that must pass
through untouched (a UUID, a git SHA, a long-but-not-secret string, a URL with no
credentials). Both directions matter: over-redaction breaks good context.

### `tools/eval/fixtures/translate.json` (T-EVAL)

`{ intent, env, profile, expected_command }` fixture pairs used by T5/T6 to
score a model/prompt change. `expected_command` is a glob string (`*` wildcard)
or a set of alternatives (OR). Seeded with 20 intents (T-104); T6 grows it
toward the top ~50 commands from the author's "I had to look this up" list, and
from `shx history`. The alias `expected_command_regex_or_set` is also accepted.

## 3. Tests that guard invariants (must never be deleted)

| ID | Invariant | Test |
| --- | --- | --- |
| **T-SAFE-3** | Print-only: no execution of user-derived commands | Greps the tree for `process::Command`, `Command::new`, `libc::system`, `execvp`, `popen`, `sh -c`; fails unless the file is allow-listed with a justifying comment. Also asserts no `--run`/`--exec` flag exists. |
| **T-CLI-1** | stdout is pure | For a fixed mock intent, stdout has no ANSI, no "explanation", no trailing prose; only the command (or the JSON). |
| **T-CLI-2** | Warnings never touch stdout | Force a risk + a backend warning; assert stdout unchanged, stderr carries both. |
| **T-CLI-3** | Exit codes | Table test over scenarios → `{0,2,3,4,5,6}` exactly per [05-CLI-SPEC.md](05-CLI-SPEC.md) §5. |
| **T-MEM-1** | Redaction at write | Record an interaction containing a fake key; read it back; assert masked. |
| **T-MEM-2** | Budget | Property test: `ContextBuilder` never exceeds `max_tokens` and never panics for arbitrary stores. |
| **T-MEM-3** | Migrations | Open a checked-in v0/v1 fixture DB, migrate, assert final `user_version` and that prior rows survive. |
| **T-MEM-4** | Cache safety | Same intent, different `cwd`/`project_id` → **no** cache hit. |
| **T-ROUTE-1** | Escalation | Mock local `Timeout` → asserts exactly one cloud call and a stderr escalation notice. |
| **T-ROUTE-2** | No silent egress | With `mode=local`, assert **zero** cloud calls on any local outcome (including failure). |
| **T-ROUTE-3** | Egress redaction | Assert the serialized cloud request body contains no known secret from a poisoned fixture. |
| **T-DET-1** | Determinism | Prompt built twice from identical inputs is byte-identical (golden). |

## 4. Test doubles

- `MockBackend`: fixture-driven, supports fault injection
  (`Timeout | Unreachable | Auth | ModelMissing | BadOutput | RateLimit`) and
  scripted confidence values — this is how the router is tested network-free.
- `InMemoryStore`: full `MemoryStore` impl over `Vec`/`HashMap` — the default in
  T1/T2/T3 so unit tests need no temp files.
- `FakeClock`: injectable "now" so decay/retention/`ts` assertions are stable and
  not flaky.

## 5. CI gate (`cargo xtask ci`)

Exactly what CI runs, so local == remote:

1. `cargo fmt --all --check`
2. `cargo clippy --all-targets --all-features -- -D warnings`
3. `cargo test --workspace --all-features` (T1–T4)
4. `cargo clippy --no-default-features` + test (privacy-minimal build still works)
5. `cargo deny check` (licenses, advisories, bans, sources)
6. `cargo audit`
7. `typos`
8. `cargo doc --no-deps --deny warnings`
9. `cargo build --release` and a size check (warn > 15 MB, fail > 25 MB)

CI matrix: **ubuntu-latest, macos-14 (arm64), windows-latest**, stable Rust (pinned
in `rust-toolchain.toml`). All three OSes from M0 — cross-platform is a build
target, not a claim.

## 6. Performance tests

- `benches/startup.rs`: assert `shx --version` / `shx doctor --json` under a
  time budget (a regression here is a product regression for a terminal tool).
- `benches/classifier.rs`: `criterion` over the corpora — the classifier must stay
  sub-millisecond.
- Cache-hit path: assert < 5 ms for a hit in a loop.

Perf tests run in CI in "report" mode (no hard gate initially), with a hard gate
added once baseline numbers exist.

## 7. Eval harness (T6)

`tools/eval` runs `translate.json` fixtures through a chosen backend and reports:

- exact-match rate, regex-match rate, "acceptable but different" rate,
- risk misclassification count (a returned danger command we scored Safe),
- p50/p95 latency, token usage, cache-hit rate.

This is how prompt/model changes are judged objectively instead of by vibes, and
it feeds the README's honest benchmark table. Any PR touching
`shx-core/src/prompt.rs` or a backend must include an eval delta in its
description.

## 8. What we deliberately don't test

- Model *quality* in unit tests (that's T6's job, and it needs a real model).
- Exact model output text (unstable) — we assert on structure and regexes.
- Cloud providers' own behavior (contract: the `BackendError` normalization).

## 9. Flake policy

Any test that fails intermittently is quarantined (`#[ignore]` + an issue) within
a day and fixed or deleted within a week. A flaky suite trains agents and humans
to ignore red, which is worse than a smaller suite.
