# 02 — Architecture

## 1. Shape of the system

`shx` is a **single binary** built from a **Cargo workspace** of small library
crates. The workspace exists so that multiple agents (and later contributors) can
work on independent modules behind frozen interfaces, and so the pure logic
(risk classification, prompt assembly, retrieval) is unit-testable without
network, disk, or a model.

There is **no daemon, no server, no background process**. One process starts,
does work, prints, exits.

```
┌──────────────────────────────────────────────────────────────────┐
│ shx-cli  (bin: shx)  — clap parsing, rendering, exit codes        │
└───────────────┬──────────────────────────────────────────────────┘
                │ orchestrates the 6-step pipeline
                v
┌──────────────────────────────────────────────────────────────────┐
│ shx-core  — domain types + pure pipeline stages                   │
│   • Intent          • ContextBundle     • Prompt  (pure build)    │
│   • TranslateRequest/Response  • Candidate  • RiskAssessment      │
│   • RiskClassifier (pure, no IO)   • Redactor (pure, no IO)       │
└───────┬──────────────────────┬──────────────────────┬────────────┘
        │                      │                      │
        v                      v                      v
┌───────────────┐    ┌──────────────────┐    ┌──────────────────┐
│ shx-llm       │    │ shx-memory       │    │ shx-config       │
│ Backend trait │    │ SQLite store +   │    │ layered config   │
│ ollama/anthropic│  │ retrieval +      │    │ (defaults, TOML, │
│ /openai-compat  │  │ migrations       │    │  env, flags)     │
│ /mock         │    │ (rusqlite)       │    │                  │
└───────────────┘    └──────────────────┘    └──────────────────┘
        │                      │                      │
        └──────────────────────┴──────────────────────┘
                     dep direction: inward only
        shx-cli → {core, llm, memory, config}
        {llm, memory, config} → core   (core depends on none of them)
```

**Dependency rule:** `shx-core` is the inward layer and depends on *no* other
`shx-*` crate. Everything depends inward. `shx-core` has no `tokio`, no
`rusqlite`, no `reqwest`. This is what keeps the logic pure and portable.

## 2. Crate-by-crate

### `shx-core` (lib, no IO)

The domain. Everything here is a pure function of its inputs.

- `types.rs` — `Intent`, `ContextBundle`, `TranslateRequest`, `TranslateResponse`,
  `Candidate { command, explanation, confidence }`, `RiskAssessment`,
  `RiskLevel { Safe, Review, Danger }`.
- `prompt.rs` — `PromptBuilder`: builds system + user messages from an `Intent`
  and a `ContextBundle`. Deterministic; golden-tested.
- `risk.rs` — `RiskClassifier`: `&str → RiskAssessment`. Pure regex + heuristic
  rules, no allocation beyond the report. See [04-SAFETY.md](04-SAFETY.md).
- `redact.rs` — `Redactor`: pattern-based secret masking for text destined for
  a backend or the memory store. Pure. See [04-SAFETY.md](04-SAFETY.md).
- `parse.rs` — parses the backend's JSON contract into `TranslateResponse`,
  tolerant of fenced code blocks and minor model noise.

### `shx-llm` (lib)

- `backend.rs` — the `Backend` trait (the frozen interface; see §4).
- `ollama.rs`, `anthropic.rs`, `openai_compat.rs`, `mock.rs`.
- `router.rs` — `BackendRouter` implementing the local-first escalation policy.
- `http.rs` — thin request helper (v1: sync `ureq`, feature-gated; ADR-006).

### `shx-memory` (lib)

- `store.rs` — `MemoryStore` over `rusqlite` (bundled SQLite, WAL).
- `migrations/` — versioned SQL, applied via `PRAGMA user_version`.
- `retrieve.rs` — recency + relevance selection, token budgeting.
- `record.rs` — write path (interaction, vocabulary, feedback, snippet).
- `paths.rs` — XDG-correct DB location via `directories`.

Schema and behavior: [03-MEMORY.md](03-MEMORY.md).

### `shx-config` (lib)

- `model.rs` — typed config structs mirroring [05-CLI-SPEC.md §Config](05-CLI-SPEC.md).
- `load.rs` — layered merge: built-in defaults → global → project → env → flags.
- `validate.rs` — semantic validation with actionable errors (`shx doctor` reuses
  it).

### `shx-cli` (bin `shx`)

- `main.rs` — clap command tree, dispatch.
- `pipeline.rs` — the orchestrator (see §3).
- `render.rs` — human vs `--json` output, color handling, risk banners.
- `commands/` — `doctor`, `history`, `snippets`, `config`, `teach`, `feedback`,
  `import_history`, `completion`.

### `xtask` (bin, dev-only)

`cargo xtask ci` runs the full local gate: `fmt --check`, `clippy -D warnings`,
`test`, `cargo-deny`, `typos`, doc build. Agents and humans run the same thing CI
runs.

## 3. The request pipeline

The one and only path from intent to printed command. Each stage is a seam for
tests.

```
 argv (natural language)
   │
   ├─1. parse & load config            shx-cli + shx-config
   │      Intent { text, force_backend, count, flags }
   │
   ├─2. build context                  shx-memory + shx-core
   │      ContextBundle {
   │        env: os, shell, cwd, git_root, in_container,
   │        profile: ports, docker_pref,
   │        history: Vec<Interaction>   ← recency + relevance, token-budgeted
   │        vocabulary: Vec<(term, expansion)>,
   │        snippets: Vec<Snippet>
   │      }
   │      ↳ redactor runs on history/vocabulary before it can reach a backend
   │
   ├─3. fast-path cache check          shx-memory
   │      exact (normalized intent + context fingerprint) hit?
   │        → skip to step 6
   │
   ├─4. translate                      shx-llm router
   │      PromptBuilder(ContextBundle, Intent) → TranslateRequest
   │      router picks backend(s):
   │        local → if Err | timeout | confidence < threshold → escalate cloud
   │      TranslateResponse { candidates, raw, usage, backend_id, latency }
   │
   ├─5. classify & annotate            shx-core
   │      for each candidate: RiskClassifier → RiskAssessment
   │      Redactor already applied pre-egress; classification is post-response
   │
   ├─6. select & record & render        shx-cli + shx-memory
   │      pick top candidate (or all if -n)
   │      record Interaction {…, accepted: null, executed: null, risk_level}
   │      render: stdout = command ONLY (script-safe);
   │              stderr = explanation, confidence, risk banners, --why
   │      exit code per spec (0 / 3 with --exit-on-risk / 4 / 6 …)
```

**Critical output contract:** the resolved command goes to **stdout alone**,
with no decoration, so `shx "…" | pbcopy` and `CMD=$(shx "…")` work. All human
chrome — explanation, confidence, warnings — goes to **stderr**. `--json` is the
exception: one JSON object on stdout, nothing on stderr unless it's an error.

## 4. Frozen interfaces (change only via ADR)

These three traits are the contracts that let tasks parallelize. Changing a
signature requires an ADR + a version bump + notifying dependent tasks
(see [docs/agents/AGENT-GUILD.md](agents/AGENT-GUILD.md), "Interface freeze").

### `Backend`

```rust
pub trait Backend: Send + Sync {
    fn id(&self) -> &'static str;               // "ollama", "anthropic", …
    fn capabilities(&self) -> Capabilities;     // context_len, structured_output, streaming
    fn health(&self) -> Health;                 // cheap reachability probe (doctor)
    fn translate(&self, req: &TranslateRequest) -> Result<TranslateResponse, BackendError>;
}
```

v1 is deliberately **synchronous** (ADR-006). If streaming lands, it arrives as a
separate `translate_stream` with a default impl that falls back to `translate`.

### `MemoryStore` (trait over the SQLite impl, enabling an in-memory/test double)

```rust
pub trait MemoryStore {
    fn record_interaction(&self, i: &Interaction) -> Result<i64>;
    fn recent(&self, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;
    fn search(&self, query: &str, limit: usize, scope: Scope) -> Result<Vec<Interaction>>;
    fn vocabulary(&self, terms: &[String]) -> Result<Vec<VocabEntry>>;
    fn upsert_vocabulary(&self, e: &VocabEntry) -> Result<()>;
    fn snippets(&self) -> Result<Vec<Snippet>>;
    fn feedback(&self, interaction_id: i64, v: Verdict, note: Option<&str>) -> Result<()>;
    fn prune(&self, policy: &PrunePolicy) -> Result<PruneReport>;
}
```

`Scope` is the memory scope from [03-MEMORY.md](03-MEMORY.md) — it is what allows
"tool history only" and "also shell history" to coexist behind one API.

### `Redactor`

```rust
pub trait Redactor {
    fn redact(&self, input: &str) -> Redacted<'_>;   // Cow: borrowed if untouched
}
```

## 5. Concurrency & performance budget

- Single-threaded execution is enough for v1. No `tokio` runtime in the hot path
  (ADR-006). If a cloud backend needs concurrency later, add it inside `shx-llm`
  only.
- **Startup budget:** ≤ 30 ms to first work on the reference machine. Practices:
  lazy DB open (only if memory is enabled and needed), lazy regex compilation
  (`OnceLock`), no global allocator games, `strip = true`, `lto = "thin"`,
  `codegen-units = 1` for release.
- **Translation budget:** local path p50 < 2.5 s end-to-end on `qwen3:14b`;
  fast-path cache hit < 5 ms.
- **Binary budget:** < 15 MB stripped on macOS arm64 with the default feature set;
  a `--no-default-features` build drops cloud backends and `--copy`.

## 6. Error model

- `thiserror` enums per crate; `anyhow` only in `shx-cli` where context strings
  help the human.
- No panics on user input. No `unwrap` outside tests and `expect`-with-reason for
  truly-impossible states.
- Every backend error is **normalized** into `BackendError { kind, backend,
  retryable, detail }` so the router can decide to escalate without knowing which
  provider it's talking to.
- Errors render to stderr as one actionable line, optionally `--verbose` for the
  full chain (never to stdout — stdout stays script-safe).

## 7. Feature flags

```toml
[features]
default = ["cloud", "clipboard"]
cloud     = []          # anthropic + openai-compat
clipboard = ["dep:arboard"]
```

Local-only builds (`--no-default-features`) are the privacy-maximal, smallest
artifact. The `Backend` trait stays in the default build either way; only the
implementations are gated. `mock` is always compiled (tests depend on it).

## 8. Repo layout

```
shx/
├── Cargo.toml                 # workspace
├── rust-toolchain.toml        # pinned stable
├── xtask/                     # cargo xtask ci
├── crates/
│   ├── shx-core/
│   ├── shx-llm/
│   ├── shx-memory/
│   ├── shx-config/
│   └── shx-cli/
├── tests/                     # workspace-level integration (assert_cmd)
│   ├── fixtures/              # golden prompts, backend recordings, corpora
│   └── corpus/                # risk + redaction labeled corpora
├── docs/                      # this documentation set
│   └── agents/                # agent guild + task board
├── .github/                   # CI, PR/issue templates
├── CONTRIBUTING.md
├── CODE_OF_CONDUCT.md
├── AGENTS.md
└── LICENSE-MIT / LICENSE-APACHE
```

## 9. What is deliberately *not* here

- No execution module. No `std::process::Command` with a user-derived program
  name anywhere in the tree, except `shx doctor` probing `ollama` and
  `shx import-history` reading a history file. This is asserted by a test that
  greps the source (see [08-TESTING.md](08-TESTING.md), T-SAFE-3).
- No plugin system in v1 (an MCP server mode is the sanctioned extension point,
  P2).
- No async runtime, no daemon, no background threads.
