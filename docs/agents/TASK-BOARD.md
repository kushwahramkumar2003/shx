# Task Board

Build order for `shx`. Read [AGENT-GUILD.md](AGENT-GUILD.md) for the codeship
cycle, DoR, and DoD. The machine-readable mirror is [tasks.yaml](tasks.yaml) —
keep the two in sync (a Docs task updates both).

**How to use this board**

1. Pick any task whose `deps` are all `completed`, whose files don't collide with
   an `in_progress` task, and whose role you can claim.
2. Follow the codeship cycle. One task per branch, one role per task.
3. Do not start a task whose `serialize: true` prerequisite isn't merged.

Legend: `S`/`M`/`L` = rough size · `∥` = parallel-safe within its wave ·
`⛓` = serialization point.

---

## Waves at a glance

| Wave | Milestone | Tasks | Notes |
| --- | --- | --- | --- |
| 0 | M0 | T-001 | Bootstrap. Single agent; everything depends on it. |
| 1 | M0 | T-002, T-003 ⛓, T-004 | Infra + contracts + config. |
| 2 | M0 | T-005, T-006, T-007, T-008 | First end-to-end offline path. |
| 3 | M1 + M3 | T-101 ⛓, T-102, T-103, T-104, T-301, T-302, T-303 | Local translations **and** the safety core (classifier + redactor land early because memory's write path depends on the redactor). |
| 4 | M2 | T-201 ⛓, T-202, T-203, T-204, T-205, T-206 | Memory. |
| 5 | M3 | T-304, T-305 | Safety UX (needs the wave-3 core). |
| 6 | M4 | T-401, T-402, T-403, T-404, T-405 | Cloud + routing. |
| 7 | M5 | T-501…T-506 | Context awareness + learning. |
| 8 | M6 | T-601…T-608 | UX + packaging. |

Recommended concurrency: **3–4 agents** per wave. Serialization points (`⛓`) run
first in their wave and block the rest until merged.

---

## Wave 0 — Bootstrap

### T-001 — Workspace scaffold ⛓ · Release · L

**Deliverables:** `Cargo.toml` (workspace), `rust-toolchain.toml`,
`crates/{shx-core,shx-llm,shx-memory,shx-config,shx-cli}/Cargo.toml`,
`xtask/`, `.gitignore`, `rustfmt.toml`, `deny.toml`, `LICENSE-MIT`,
`LICENSE-APACHE`, empty `lib.rs`/`main.rs` that compile.

**Acceptance:** `cargo build --workspace` and `cargo test --workspace` succeed
(0 tests) on all three OSes; `cargo xtask ci` exists and runs the step list from
[08-TESTING.md](../08-TESTING.md) §5; `cargo run -p shx -- --version` prints a
version.

**Notes:** Pin the toolchain. Add the crate dependency rule as a doc comment in
each `Cargo.toml` (depends inward only). Do **not** add `tokio`/`reqwest`
(ADR-006). This is the wave's serialization point.

---

## Wave 1 — Contracts

### T-002 — CI matrix + xtask verification · Release · M

**Deps:** T-001 · ∥

**Deliverables:** `.github/workflows/ci.yml` (ubuntu/macos-14/windows, stable),
`xtask/src/main.rs` implementing the 9-step gate, `cargo-deny` config,
`typos.toml`.

**Acceptance:** CI green on all three OSes for a no-op PR; a deliberately-introduced
clippy warning fails CI; a deliberately-introduced typo fails CI.

### T-003 — Core types & frozen traits ⛓ · Architect · M

**Deps:** T-001

**Deliverables:** `shx-core/src/types.rs` (`Intent`, `ContextBundle`, `Candidate`,
`RiskAssessment`, `RiskLevel`, `TranslateRequest`, `TranslateResponse`,
`Usage`, `Interaction`, `Scope`), `shx-llm/src/backend.rs` (`Backend`,
`Capabilities`, `Health`, `BackendError`, `ErrorKind`),
`shx-memory/src/store.rs` (the `MemoryStore` trait + `InMemoryStore` stub),
`shx-core/src/redact.rs` (the `Redactor` trait + no-op impl).

**Acceptance:** the exact signatures in [02-ARCHITECTURE.md](../02-ARCHITECTURE.md)
§4 compile; doc comments on every public item; a test constructs each type and
round-trips through serde where applicable.

**Notes:** ⛓ Serialization point for the whole project. Get Architect sign-off;
this PR *is* the interface. Everything else consumes it.

### T-004 — Config loader · Core · M

**Deps:** T-001 · ∥

**Deliverables:** `shx-config/src/{model.rs,load.rs,validate.rs}` implementing the
layered merge and every key in [05-CLI-SPEC.md](../05-CLI-SPEC.md) §4.

**Acceptance:** unit tests for precedence (defaults < global < project < env <
flags), unknown-key → warning (not error), invalid value → error naming the key
path and accepted set; `SHX_BACKEND__MODE=cloud` overrides nested key; a
`shx.toml` fixture parses into the typed struct.

---

## Wave 2 — First end-to-end offline path

### T-005 — Mock backend · QA · S

**Deps:** T-003 · ∥

**Deliverables:** `shx-llm/src/mock.rs` — fixture-driven `TranslateResponse`,
fault injection (`Timeout|Unreachable|Auth|ModelMissing|BadOutput|RateLimit`),
scripted confidence.

**Acceptance:** unit test drives each injected fault and asserts the exact
`BackendError { kind, retryable }`; determinism test (same input → same output).

### T-006 — CLI skeleton + output discipline · CLI/UX · M

**Deps:** T-003, T-004 · ∥

**Deliverables:** `shx-cli/src/main.rs` (clap tree for the primary invocation +
every subcommand as a stub), `pipeline.rs` wired to `MockBackend` + no memory,
`render.rs` with the stdout/stderr split.

**Acceptance:** `shx --offline "run pg on 7000"` prints exactly one fixture
command to stdout with nothing else (**T-CLI-1**); warnings/errors go to stderr
(**T-CLI-2**); `--json` emits the shape in [05-CLI-SPEC.md](../05-CLI-SPEC.md) §6;
exit codes per table (**T-CLI-3**).

### T-007 — `shx doctor` skeleton + `config init` · CLI/UX · M

**Deps:** T-004, T-005 · ∥

**Deliverables:** `shx-cli/src/commands/doctor.rs`, `commands/config.rs`, the
commented default config template.

**Acceptance:** `doctor --json` reports config-parse, DB-path (stub), backend
reachability (mock), and effective config; `config init` writes a file that
`validate` accepts; `config path` prints discovered paths in precedence order.

### T-008 — Print-only guard test (T-SAFE-3) · Safety · S

**Deps:** T-001 · ∥

**Deliverables:** `tests/invariants/print_only.rs` — greps the tree for
`process::Command`, `Command::new`, `std::process`, `libc::system`, `execvp`,
`popen`, `sh -c`; allow-lists `shx doctor`'s probe and `config edit` with
justifying comments; asserts no `--run`/`--exec` flag exists in the CLI.

**Acceptance:** test passes on the current tree; a test-only fixture that adds a
`Command::new` in a non-allow-listed file makes it fail (verified by temporarily
enabling the fixture under `#[ignore]` docs, or by a string fixture).

**Notes:** This test is a permanent invariant. Never delete it (ADR-002).

---

## Wave 3 — M1 Local backend + M3 safety core

> The safety core (T-301/T-302/T-303) belongs in this wave despite being an M3
> item: it has no backend dependency, and memory's write path (T-202) cannot
> start without the redactor. Landing it early unblocks Wave 4.

### T-101 — Prompt builder v1 + response parser ⛓ · Core · M

**Deps:** T-003

**Deliverables:** `shx-core/src/prompt.rs` (`PromptBuilder` per
[06-BACKENDS.md](../06-BACKENDS.md) §5), `shx-core/src/parse.rs` (fence
stripping, first-balanced-JSON extraction, schema validation → `BadOutput`).

**Acceptance:** golden test asserts byte-identical prompts for fixed inputs
(**T-DET-1**); parser tests cover valid JSON, fenced JSON, prose-wrapped JSON,
truncated JSON (→ `BadOutput`), wrong types (→ `BadOutput`), and extra keys
(tolerated).

### T-102 — Ollama backend · Backend · M

**Deps:** T-003, T-101 (for schema), T-004

**Deliverables:** `shx-llm/src/{ollama.rs,http.rs}` per
[06-BACKENDS.md](../06-BACKENDS.md) §4.

**Acceptance:** `--offline` stays green (no network in tests); `mock`-based
request-body unit test asserts `keep_alive`/`num_ctx`/schema are sent;
`health()` distinguishes reachable+present, reachable+missing (→ `ModelMissing`
with the `ollama pull` hint), unreachable; live smoke test is `#[ignore]`d and
documented.

### T-103 — Error normalization + doctor backend checks · Backend · S

**Deps:** T-102

**Deliverables:** map provider errors → `BackendError`; extend `doctor` to run
real probes.

**Acceptance:** table test maps each provider condition to the right
`ErrorKind`/`retryable`; `doctor` exit code is 4 when the local backend is
unreachable.

### T-104 — Live test harness (T5) · QA · S

**Deps:** T-102 · ∥

**Deliverables:** `tests/live/` gated by `SHX_LIVE=1`, `tools/eval/fixtures`
seed with 20 intents.

**Acceptance:** skipped by default in CI; documented run instructions; a live run
prints a small results table.

**Notes:** `SHX_LIVE=1 cargo test -p shx-llm --test live -- --ignored --nocapture`.
Fixtures: `tools/eval/fixtures/translate.json`. Run instructions in
[08-TESTING.md](../08-TESTING.md) § T5.

### T-301 — Risk classifier + rule table · Safety · L

**Deps:** T-003 · ∥

**Deliverables:** `shx-core/src/risk.rs` with the rule families in
[04-SAFETY.md](../04-SAFETY.md) §2 as data (`{ id, family, level, matcher }`).

**Acceptance:** the full rule table is data-driven; sub-millisecond on the corpora
(bench); no panics on arbitrary input (property test); rule ids stable and
documented.

### T-302 — Danger + benign corpora tests · Safety · M

**Deps:** T-301 · ∥

**Deliverables:** `tests/corpus/risk_danger.txt`, `risk_benign.txt`, and the tests
**T-SAFE-1** (zero false negatives) / **T-SAFE-2** (<10% benign FP).

**Acceptance:** danger corpus has ≥ 40 lines spanning every family; the FP test
prints the flagged benign set on failure; adding a rule without a corpus line
fails a meta-test.

### T-303 — Redactor (full pattern set) + corpus · Safety · M

**Deps:** T-003 · ∥

**Deliverables:** `shx-core/src/redact.rs` full impl per
[04-SAFETY.md](../04-SAFETY.md) §3.1, `tests/corpus/redaction.txt`.

**Acceptance:** **T-SAFE-4** — every positive pattern masks; every negative case
(UUID, git SHA, long non-secret) passes through untouched; `Cow` avoids allocation
when nothing matches (bench/assert).

---

## Wave 4 — M2 Memory

### T-201 — Memory store + migrations v1 ⛓ · Memory · L

**Deps:** T-003

**Deliverables:** `shx-memory/src/{store.rs,paths.rs,migrations/001_init.sql}`,
SQLite `MemoryStore` impl (bundled, WAL, `user_version`), `InMemoryStore` full
impl, `Scope` filtering.

**Acceptance:** **T-MEM-3** (migration from a checked-in v0 fixture DB, rows
survive); `InMemoryStore` and SQLite impl pass the *same* conformance suite;
permissions are `0600`/`0700` on Unix; `prune` applies `retention_days` and the
danger-command 30-day rule.

### T-202 — Write path + redaction-at-write · Memory · M

**Deps:** T-201, T-303 (redactor — landed in Wave 3)

**Deliverables:** `shx-memory/src/record.rs`; redaction applied to `input_nl`,
`output_cmd`, shell history before insert.

**Acceptance:** **T-MEM-1** (record a fake key → read back masked); no code path
writes a raw secret; `record` is called by the pipeline with `from_cache`,
`risk_level`, `latency_ms` populated.

### T-203 — Context builder v1 · Memory · M

**Deps:** T-201

**Deliverables:** `shx-memory/src/retrieve.rs` per
[03-MEMORY.md](../03-MEMORY.md) §4 (anchor + relevance + vocabulary +
snippets + budget).

**Acceptance:** **T-MEM-2** (property: never exceeds `max_tokens`, never panics;
exact bundle for fixed fixtures — order, dedupe, truncation); determinism test;
redaction ran on every block.

### T-204 — `shx history` family · CLI/UX · M

**Deps:** T-201

**Deliverables:** `history [list|show|export|prune|purge]` with `--json/--jsonl`,
filters (`--project`, `--risk`, `--grep`).

**Acceptance:** CLI tests over a seeded temp DB; `purge` requires confirmation
unless `-y`; `export --jsonl` round-trips through import; exit codes match spec.

### T-205 — `shx teach` + vocabulary learning · Memory · M

**Deps:** T-201, T-203

**Deliverables:** `teach`/`--forget`/`--list`; learned-weight update on `good`
feedback; decay at prune; the "apply only when weight ≥ 1.5" rule.

**Acceptance:** taught beats learned on conflict; one-off coincidences don't
become vocabulary; decay is deterministic with `FakeClock`.

### T-206 — `--why` explainability · CLI/UX · S

**Deps:** T-203, T-204

**Deliverables:** stderr block listing used memory entries (id + one-line), profile
fields, budget/truncation, and the routing trace placeholder (filled in T-403).

**Acceptance:** snapshot test of the `--why` output for a fixed fixture; `--why`
never writes to stdout.

---

## Wave 5 — M3 Safety UX

### T-304 — Risk UX: banners, exit codes, `--exit-on-risk` · CLI/UX · M

**Deps:** T-301, T-303, T-006

**Deliverables:** color-coded stderr banners (what + why), exit code 3 with
`--exit-on-risk`, exit code 6 for refused intents, `refuse_multi_command_on_risk`.

**Acceptance:** **T-CLI-2** still green (stdout pure); snapshot of a `Danger`
banner; exit-code table test extended.

### T-305 — `doctor --redaction-test` · CLI/UX · S

**Deps:** T-303

**Deliverables:** run the redaction corpus and report coverage/counts.

**Acceptance:** reports per-pattern pass/fail; exit 1 if any pattern fails.

---

## Wave 6 — M4 Cloud + routing

### T-401 — Anthropic backend · Backend · M

**Deps:** T-003, T-101, T-004

**Deliverables:** `shx-llm/src/anthropic.rs` (forced tool-use structured output).

**Acceptance:** request-body unit test asserts the single-tool `input_schema`;
response mapping into `TranslateResponse`; auth failure → `ErrorKind::Auth`;
api key read from the env var *named* in config, never from the file.

### T-402 — OpenAI-compatible backend + capability probe · Backend · M

**Deps:** T-401 (shared patterns) · ∥

**Deliverables:** `shx-llm/src/openai_compat.rs`; `json_schema` ↔ `json_object`
probe cached in `meta`.

**Acceptance:** probe downgrade path tested with two mock servers (one rejecting
`json_schema`); works against a `base_url` with no key (LM Studio).

### T-403 — BackendRouter (local-first escalation) ⛓ · Backend · M

**Deps:** T-102, T-401, T-402

**Deliverables:** `shx-llm/src/router.rs` implementing
[06-BACKENDS.md](../06-BACKENDS.md) §3 (escalate on error/timeout/bad-output/
low-confidence; one attempt; visible notice).

**Acceptance:** **T-ROUTE-1** (mock local timeout → exactly one cloud call + one
stderr notice); **T-ROUTE-2** (`mode=local` → zero cloud calls on *any* local
outcome); confidence thresholds honored; `--why` routing trace populated.

### T-404 — Egress redaction + route security tests · Safety · M

**Deps:** T-303, T-403

**Deliverables:** redaction inserted before serializing any non-loopback request;
**T-ROUTE-3** over a poisoned fixture.

**Acceptance:** the serialized cloud request body contains no known secret;
redaction is asserted for both cloud and local paths (defense in depth).

### T-405 — `--local` / `--cloud` flags · CLI/UX · S

**Deps:** T-403

**Deliverables:** flags; `--cloud` unconfigured → usage error (exit 2).

**Acceptance:** CLI table test; forcing cloud with no key never silently falls
back to local.

**Status:** completed (local-only). Completes Wave 6 (M4).

---

## Wave 7 — M5 Context awareness & learning

### T-501 — Project scoping + container detection · Memory · M

**Deps:** T-201, T-203

**Deliverables:** `project_id` from git root (hash), `in_container` detection,
`ContextBundle.env` population; `--project` override.

**Acceptance:** two different repos produce distinct `project_id`s and disjoint
project-scoped recall; detection works inside/outside a container (mocked FS).

### T-502 — Fast-path cache · Memory · M

**Deps:** T-203, T-301

**Deliverables:** cache lookup keyed by normalized intent + context fingerprint;
eligibility rules from [03-MEMORY.md](../03-MEMORY.md) §5.

**Acceptance:** **T-MEM-4** (same intent, different cwd/project → no hit);
cache-hit path < 5 ms (bench); cache never stores `danger` results; `from_cache`
surfaced in `--json`/`--why`.

### T-503 — Feedback loop + weights/decay · Memory · M

**Deps:** T-205

**Deliverables:** `shx feedback` subcommand; weight updates; `accepted`/`executed`
columns updated.

**Acceptance:** `good` raises weights (cap 3.0), `bad` lowers or does not raise;
`--executed` sets `executed=1`; decay deterministic.

### T-504 — Snippets · CLI/UX · M

**Deps:** T-201

**Deliverables:** `shx snippet save|list|show|rm`; snippets enter the
`ContextBundle`.

**Acceptance:** snippet CRUD + `--copy` from `show`; snippet matching appears in
`--why`; snippets never auto-execute as a bare `shx <name>` (documented).

**Status:** completed (local-only). Unblocks **T-505**.

### T-505 — BM25 relevance upgrade · Memory · M

**Deps:** T-203, T-504

**Deliverables:** replace keyword scoring in step 2 of the context builder with
BM25 over the interaction corpus.

**Acceptance:** golden bundle tests updated and reviewed; improvement measured on
the eval harness (T6) — PR includes the delta; determinism preserved.

**Status:** completed (local-only). Unblocks **T-506**.

### T-506 — Eval harness (T6) · QA · L

**Deps:** T-104, T-505

**Deliverables:** `tools/eval` scoring `translate.json` (exact/regex/acceptable
rates, risk-misclassification count, latency, cache-hit rate); CI job on prompt/
backend paths.

**Acceptance:** running it produces the metrics table used in the README;
a prompt regression that drops exact-match rate is visible in one command.

---

## Wave 8 — M6 UX & packaging

| Task | Role | Deps | Deliverable | Acceptance |
| --- | --- | --- | --- | --- |
| **T-601** `--explain` | CLI/UX | T-101 | reverse-mode prompt + rendering | explain mode returns prose to stderr, nothing to stdout except with `--json` |
| **T-602** `-i` refine session | CLI/UX | T-201, T-403 | multi-turn loop, `session_id` | session groups interactions; final command recorded once; Ctrl-C exits clean |
| **T-603** `-n` candidates + `--copy` | CLI/UX | T-006 | one command per stdout line; clipboard behind feature | `-n 3` prints 3 lines to stdout; `--copy` works with `clipboard` feature, absent without it |
| **T-604** completions + man | CLI/UX | T-006 | `shx completion <shell>`, `shx man` | generated output valid for zsh/bash/fish/powershell; shipped in archive |
| **T-605** `import-history` | CLI/UX | T-202 | opt-in ingest with `--dry-run` | redacts before insert; summary printed; `--dry-run` writes nothing |
| **T-606** shell wrappers | Docs/DevRel | T-304, T-603 | `contrib/shell/shx.zsh`, `shx.bash` | ≤30 lines each; respect exit 3/4; never `eval`; documented install |
| **T-607** packaging (cargo-dist) | Release | T-002 | 5 target artifacts + brew/scoop/winget manifests | release workflow builds all targets; binary size gate met |
| **T-608** docs site + CHANGELOG | Docs/DevRel | T-607 | mdbook config + `CHANGELOG.md` | `mdbook build` passes; every doc link resolves; CHANGELOG has `[Unreleased]` |

---

## Cross-cutting tasks (run alongside any wave)

| Task | Role | Deliverable |
| --- | --- | --- |
| **T-X01** Doc truthfulness sweep | Docs/DevRel | After each wave: verify `docs/` matches behavior; open fix tasks for drift |
| **T-X02** Dependency hygiene | Release | `cargo-deny`/`audit` on every PR; quarterly dep review |
| **T-X03** Flake quarantine | QA | Quarantine + issue any flaky test within a day ([08-TESTING.md](../08-TESTING.md) §9) |

---

## Dependency quick-reference

```
T-001 ─┬─ T-002 ──────────────────────────────────── T-607 ── T-608
       ├─ T-003 ⛓ ─┬─ T-005 ── T-006 ── T-603 ── T-604
       │            ├─ T-101 ⛓ ─┬─ T-102 ── T-103 ── T-104
       │            │            └─ T-401 ── T-402 ── T-403 ⛓ ─┬─ T-404
       │            ├─ T-201 ⛓ ─┬─ T-202 ── T-605            └─ T-405
       │            │            ├─ T-203 ── T-206 ── T-501
       │            │            ├─ T-204
       │            │            ├─ T-205 ── T-503
       │            │            └─ T-504 ── T-505 ── T-506
       │            ├─ T-301 ── T-302
       │            ├─ T-303 ── T-305
       │            └─ T-008
       └─ T-004 ── T-007
```

Critical path: `T-001 → T-003 → T-101 → T-102 → T-403 → T-404 → T-506 → T-608`.
Keep the critical path unblocked; parallelize around it.
