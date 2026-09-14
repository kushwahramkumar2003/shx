# 09 — Roadmap

Milestones are **dependency-ordered waves**, not dates. Each milestone is a
shippable increment; M0–M3 together are a usable local product. Task-level detail
lives in [agents/TASK-BOARD.md](agents/TASK-BOARD.md).

Legend: ✅ done · 🔜 next · ⬜ planned

---

## M0 — Foundation (no model required)

**Goal:** a binary that parses args, loads config, prints a command from a
fixture backend, and passes CI on all three OSes. Everything downstream depends on
the interfaces frozen here.

- Workspace, `rust-toolchain.toml`, licenses, CI matrix, `cargo xtask`.
- `shx-core` types + `Backend`/`MemoryStore`/`Redactor` traits.
- `shx-config` layered loader + validation.
- `shx-llm::mock` + the pipeline wiring.
- `shx-cli` arg tree + stdout/stderr discipline.
- `shx doctor` (skeleton) + `shx config init`.
- T-SAFE-3, T-CLI-1/2/3 guarding the contracts from day one.

**Exit:** `shx --offline "run pg on 7000"` prints a fixture command; `cargo xtask
ci` green on macOS/Linux/Windows.

## M1 — Local backend 🔜

**Goal:** real translations from Ollama.

- `shx-llm::ollama` (chat endpoint, `keep_alive`, `num_ctx`, structured output).
- Output-contract prompt v1 + `shx-core::parse` with schema validation.
- `BackendError` normalization; `doctor` reports reachability/model presence with
  the exact `ollama pull …` hint.
- Streaming deferred (ADR-006).

**Exit:** `shx "…"` returns a correct command for the top-20 intents; p50 < 2.5s
warm; `doctor` green.

## M2 — Memory

**Goal:** persistent, useful context.

- `shx-memory`: SQLite (bundled), WAL, migrations v1, `MemoryStore` +
  `InMemoryStore`.
- Write path with redaction-at-write; `shx history` family.
- `ContextBuilder` v1 (recency + keyword relevance + vocabulary + budget).
- `shx teach`, vocabulary learning hooks, `--why`.

**Exit:** T-MEM-1..4 green; repeated shorthand resolves without re-explanation;
`--why` names the entries used.

## M3 — Safety

**Goal:** the differentiator ships.

- `RiskClassifier` (rule table) + `Redactor` (full pattern set).
- Risk banners on stderr, `--exit-on-risk`, exit code 3/6.
- Corpora: `risk_danger`, `risk_benign`, `redaction`; FP budget enforced.
- `doctor --redaction-test`.

**Exit:** T-SAFE-1/2/4 green (zero danger false-negatives, <10% benign FP).

## M4 — Cloud + routing

**Goal:** quality where it matters, privacy by default.

- `anthropic` (forced tool-use) + `openai-compat` (probing json_schema).
- `BackendRouter`: local-first escalation, confidence, one-attempt bound,
  visible escalation notice.
- Mandatory egress redaction; T-ROUTE-1/2/3 green.
- `--local` / `--cloud`.

**Exit:** ambiguous intents escalate and improve; **no** silent egress; a
redaction leak test fails the build.

## M5 — Context awareness & learning

**Goal:** it gets better the more you use it.

- Project scoping (`project_id` from git root), container detection.
- Fast-path cache with context fingerprint (T-MEM-4).
- `shx feedback` loop → vocabulary weights.
- `shx snippet` family; BM25 relevance upgrade.
- `translate.json` eval harness (T6) + first honest benchmark table.

**Exit:** cache hits < 5ms; eval delta reportable per PR; project-scoped answers
correct in two different repos.

## M6 — UX polish & packaging

**Goal:** installable by strangers.

- `--explain`, `-i` refine session, `-n` candidates, `--copy`, `--json` frozen v1.
- Shell completions + man page; `contrib/shell/` ZLE and readline wrappers
  (print-only, no `eval`).
- `shx import-history` (opt-in), `shx history prune/purge`.
- `cargo-dist` release binaries for 5 targets; brew/scoop/winget manifests.
- README quickstart, docs site (mdbook), `CHANGELOG.md`.

**Exit:** a stranger installs in one command, runs `doctor`, gets a translation.

---

## Post-v1 backlog (P2, design-ready, not scheduled)

| Item | Why it's P2 |
| --- | --- |
| Embedding retrieval (`fastembed`) | Keyword+BM25 likely enough; adds deps/model |
| MCP server mode | Rides the agent ecosystem; cheap once core is a lib |
| `--run` (execution) | Contradicts the safety story — separate opt-in wrapper at most (ADR-002) |
| Team snippets repo | Needs a sharing/permission story |
| `shx cheatsheet --md` | Nice-to-have derived artifact |
| Encrypted-at-rest DB | Needs keychain + passphrase UX (ADR-010) |
| Streaming output | Latency polish; sync is fine for one-shot |
| Shell-history semantic ingest | Embedding-dependent |
| TUI browse mode | Against "tiny and scriptable" |

## Release cadence

- SemVer. `0.x` while pre-1.0; breaking CLI/JSON/config changes bump the minor and
  are called out in `CHANGELOG.md`.
- Every release: `cargo xtask ci` green on all three OSes + eval delta recorded.
- A release is cut from `main` by tag; `cargo-dist` builds the artifacts.

## Definition of "v1.0"

All of: F-01…F-08, F-10…F-24, F-27, F-30…F-35, F-40…F-45, F-50…F-53, F-55 shipped;
success criteria in [00-VISION.md](00-VISION.md) met on the reference machine;
`docs/` matches behavior; a clean clone builds and tests with no questions asked.
