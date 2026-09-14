# 07 — Decisions (ADRs)

Architecture Decision Records. Each is one decision, its context, and its
consequences. **Status:** `Accepted` · `Superseded by ADR-NNN` · `Proposed`.

New ADRs are appended; shipped ones are never edited into a different decision
(they get superseded). Any change to a frozen interface in
[02-ARCHITECTURE.md](02-ARCHITECTURE.md) §4 requires a new ADR.

---

## ADR-001 — Print-only Rust CLI core

**Status:** Accepted

**Context.** The original v1 concept rewrote the live shell input buffer via ZLE
(zsh) / readline (bash) hooks. That requires shell-specific code, is impossible
to do identically on Windows/PowerShell, and puts the tool inside the keystroke
path of every prompt.

**Decision.** Build a standalone cross-platform Rust binary that takes
natural-language input and **prints** the resulting command to stdout. No
execution, no buffer manipulation in the core. Shell-specific in-place buffer
filling is an optional, separately installed wrapper layered on top later
(F-54), never in the binary.

**Consequences.** Far simpler, identical on every OS/shell, trivially scriptable
(`$(shx …)`, `| pbcopy`). We trade away the "magic" of auto-filling the prompt —
recoverable via wrappers if wanted. This decision is the root of the safety story
(ADR-002).

---

## ADR-002 — No execution, ever (`--run` does not exist)

**Status:** Accepted

**Context.** The real risk isn't the model being wrong; it's the model being
*confidently* destructive (`rm -rf`, `docker system prune`, killing the wrong
PID) while a human presses Enter on autopilot.

**Decision.** The core has no execution path. Not disabled — absent. No `--run`,
no `--exec`, no auto-run. Enforced by test T-SAFE-3.

**Consequences.** No confirmation UX to build; the terminal is the confirmation
step. The tool stays out of the "runs commands" threat class entirely. If
execution is ever wanted, it must be a *separate opt-in wrapper* (P2), not a flag
on the core. Revisited only if a strong case appears *and* the wrapper can keep
the core clean.

---

## ADR-003 — Memory scope: tool interactions by default, shell history opt-in

**Status:** Accepted (resolves open question #1)

**Context.** "What counts as a message?" — tool translations, or also real shell
history (`cd`, `docker ps`)? They answer different questions (your shorthand vs.
what you're doing right now) and have very different privacy weight.

**Decision.** One `Scope` type, three scopes: `Tool` (on), `Shell` (off, opt-in
via `shx import-history`), `Project` (a filter on `Tool`). Shell history is a
separate table.

**Consequences.** Default is privacy-safe and immediately useful. Power users get
session awareness by importing history. The `MemoryStore` API is scope-aware so
neither mode special-cases the other.

---

## ADR-004 — Memory storage: SQLite (bundled), not an in-memory buffer

**Status:** Accepted (resolves open question #2)

**Context.** Persistent SQLite vs. a per-session ring buffer.

**Decision.** Persistent SQLite via `rusqlite` with the `bundled` feature (no
system dependency, identical on all OSes), WAL mode, `PRAGMA user_version`
migrations. Path resolved via `directories` (XDG-correct).

**Consequences.** Memory survives sessions — the stated goal ("don't re-explain
context"). Queryability enables `history --grep`, project scoping, BM25, and the
cheatsheet backlog item. Cost: a real dependency and a migration discipline;
justified by the product's core differentiator. In-memory exists only as a *test
double*, not a mode.

---

## ADR-005 — Context strategy: token-budgeted recency + relevance

**Status:** Accepted (resolves open question #3)

**Context.** Feed the full last-100 window every prompt, or a pruned/summarized
subset?

**Decision.** Assemble a deterministic `ContextBundle`: last 10 project-scoped
interactions (anchor) + top-5 keyword/BM25-relevant interactions + last-5 shell
entries (if ingested) + matching vocabulary + matching snippets, deduped,
redacted, hard-capped at `context.max_tokens` (1500).

**Consequences.** Bounded latency and prompt size; signal stays high; `--why`
can explain exactly what was used; golden tests catch prompt drift. Rejected: a
full-100 dump (wastes context, dilutes signal, slows local inference on a 16 GB
machine) and pure-summarization (lossy, and needs another model call).

---

## ADR-006 — Synchronous HTTP (`ureq`), no async runtime in v1

**Status:** Accepted

**Context.** One request per invocation, a ~30 ms startup budget, and a binary
size target conflict with pulling in a full `tokio` runtime.

**Decision.** `shx-llm` does blocking HTTP via `ureq` (rustls, no OpenSSL). The
`Backend` trait is synchronous. The HTTP layer is hidden behind the trait so it
can be swapped for `reqwest`+`tokio` if streaming/concurrency is ever needed.

**Consequences.** Smaller binary, faster cold start, simpler code. Streaming and
parallel candidate generation are deferred. The trait remains the seam, so the
swap is contained to `shx-llm`.

---

## ADR-007 — Structured JSON output contract, not free-text parsing

**Status:** Accepted

**Context.** NL→command tools are fragile mainly at the parsing boundary.

**Decision.** Every backend is asked for JSON against a fixed schema
(`commands[]`, `risk_notes`, `assumptions`), using native structured-output
support where available (Ollama `format`, Anthropic tool-use, OpenAI
`json_schema`) and schema-in-prompt fallback elsewhere. `parse.rs` validates
before accepting; invalid → `BackendError::BadOutput` → possible escalation.

**Consequences.** Reliable multi-candidate output, machine-readable `--json`, and
a bounded response (schema-bound) that resists prompt injection. Cost: less
dialect-specific structured output forces a fallback path per backend, tested by
fixtures.

---

## ADR-008 — No embeddings in v1

**Status:** Accepted

**Context.** Semantic retrieval over history could improve relevance, at the cost
of a local embedding model/dependency.

**Decision.** v1 uses keyword/BM25 relevance. Embeddings are a P2 upgrade behind
the same `ContextBuilder` interface (replacing relevance scoring only), added with
its own migration for a vectors table.

**Consequences.** No extra deps or model management in v1; the shorthand case
(keyword-heavy) is well served. Semantic "similar intent, different words" recall
is weaker for now — acceptable, and measurable via the eval harness.

---

## ADR-009 — Complexity pre-check defaults off

**Status:** Accepted

**Context.** Skipping the local model for obviously multi-clause intents would
save a round-trip, but the heuristic may misfire and silently reduce local usage.

**Decision.** Ship `complexity_skip_local` in config, **default `false`**. Enable
only after the eval harness shows a net win.

**Consequences.** No premature optimization; a measured path to enable it.
Prevents "cloud replaces local" by accident — local-first stays the real default.

---

## ADR-010 — No encryption at rest in v1

**Status:** Accepted

**Context.** The DB may contain commands (not secrets — those are redacted at
write). Encrypting it needs a keychain integration and a passphrase/recovery
story.

**Decision.** No encryption in v1; file perms `0600`, secrets redacted at write.
Revisit as P2 (keychain-backed key) if demand appears.

**Consequences.** Simple and dependency-free; addresses the realistic threat
(another user/process) with OS file permissions. A same-user process can read the
DB — accepted, since it could equally read shell history.

---

## ADR-011 — Workspace of small crates, no execution module

**Status:** Accepted

**Context.** A single crate is simpler to start; multiple crates let agents work
in parallel and keep pure logic isolated.

**Decision.** Five library crates + a thin binary + `xtask`:
`shx-core` (pure, no IO), `shx-llm`, `shx-memory`, `shx-config`, `shx-cli`. Core
depends on nothing; everything depends inward.

**Consequences.** Parallelizable development and unit-testable pure functions;
slightly more Cargo bookkeeping. The absence of an execution module is
deliberate and structural (ADR-002).

---

## ADR-012 — Local-first routing with visible escalation

**Status:** Accepted (resolves open question #4)

**Context.** Local-only is private and fast but weaker on ambiguous input;
cloud-first is higher quality but leaks context and costs latency/money.

**Decision.** Default `mode = local-first`: local answer, escalate to cloud only
on error/timeout/bad-output/low-confidence, at most once, always announced on
stderr. `--local`/`--cloud` override per invocation. Cloud egress is redacted
(ADR-004 redaction + [04-SAFETY.md](04-SAFETY.md) §3.2).

**Consequences.** Best of both without a surprise. Escalation is observable and
testable (T-ROUTE-1/2/3). Requires a good confidence signal (heuristic fallback
in [06-BACKENDS.md](06-BACKENDS.md) §6).

---

## ADR-013 — Shell buffer-fill is an optional wrapper, deferred past v1

**Status:** Accepted (resolves open question #5)

**Context.** The v1 in-place "magic" is desirable but must not complicate the
core or break the safety model.

**Decision.** Ship `contrib/shell/` zsh (ZLE) and bash (`bind -x`) scripts after
v1, strictly opt-in, that call `shx`, respect exit code 3/4, and only write to the
edit buffer — never `eval`.

**Consequences.** Core stays simple and portable; power users get the UX back.
Wrappers are per-shell maintenance we accept, outside the release-critical path.
