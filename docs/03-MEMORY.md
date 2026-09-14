# 03 — Memory Architecture

The memory subsystem is what makes `shx` more than a thin wrapper around a
model: it captures interactions, learns shorthand, and — crucially — **feeds a
token-budgeted context window back into every prompt**, so you don't re-explain
your world each time.

This doc resolves open questions #1–#3 from the original planning notes
(see [07-DECISIONS.md](07-DECISIONS.md), ADR-003/004/005 for the rationale).

---

## 1. Three scopes of memory

The original notes asked: *what counts as a "message"?* Answer — three
**scopes**, all served by one API, distinguished by a `Scope` value:

| Scope | What it holds | Default | Privacy |
| --- | --- | --- | --- |
| `Tool` | `shx`'s own translations: intent in, command out, risk, feedback | **On** | Local file; redacted at write |
| `Shell` | Imported real shell history (`cd`, `docker ps`, …) | **Off**, opt-in via `shx import-history` | Local only, never sent to cloud raw |
| `Project` | The `Tool` scope filtered to one git root (`project_id`) | On (a *view*, not a store) | Local |

`Project` is not a separate table — it's a filter on `interactions.project_id`.
`Shell` *is* a separate table (`shell_history`) because a shell command never has
a paired intent or candidate.

**Why both, behind one `Scope`:** the tool-interaction history gives the model
your *shorthand and preferences*; shell history gives it *what you're doing right
now* (e.g. you just `cd`'d into a service dir and ran `docker compose up`, so
"tail the logs" should resolve to that service). They answer different questions,
so the default is tool-only and shell ingest is a deliberate opt-in.

## 2. Storage: SQLite, single file, WAL

- **Crate:** `rusqlite` with the `bundled` feature — SQLite is compiled in, so
  there is no system dependency and behavior is identical on macOS/Linux/Windows.
- **Path:** `~/.local/share/shx/shx.db` on Linux, `~/Library/Application
  Support/shx/shx.db` on macOS, `%LOCALAPPDATA%\shx\shx.db` on Windows (resolved
  via the `directories` crate, XDG-correct). Overridable with `[memory] path`.
- **Pragmas on open:** `journal_mode=WAL` (concurrent reads, crash-safe),
  `synchronous=NORMAL`, `foreign_keys=ON`, `busy_timeout=5000`.
- **Migrations:** `PRAGMA user_version` + numbered SQL files in
  `crates/shx-memory/src/migrations/`. Each migration is forward-only and
  tested (open a vN DB → migrate → assert vN+1). Never rewrite a shipped
  migration; add the next one.
- **Concurrency:** the CLI is short-lived, so a single connection per process is
  fine. WAL means a long-running `shx history --follow` (P2) won't block writes.

## 3. Schema (v1)

```sql
-- 001_init.sql  → user_version = 1

CREATE TABLE interactions (
  id            INTEGER PRIMARY KEY,
  ts            INTEGER NOT NULL,              -- unix millis, UTC
  session_id    TEXT    NOT NULL,              -- per-process uuid, groups a chat/refine run
  project_id    TEXT,                          -- hash of git root, NULL outside a repo
  cwd           TEXT    NOT NULL,
  os            TEXT    NOT NULL,              -- "macos" | "linux" | "windows"
  shell         TEXT    NOT NULL,              -- "zsh" | "bash" | "fish" | "powershell" | …
  input_nl      TEXT    NOT NULL,              -- redacted
  output_cmd    TEXT    NOT NULL,
  explanation   TEXT,
  backend       TEXT    NOT NULL,              -- "ollama" | "anthropic" | …
  model         TEXT    NOT NULL,
  confidence    REAL,                          -- 0.0–1.0, NULL if unavailable
  latency_ms    INTEGER NOT NULL,
  risk_level    TEXT    NOT NULL,              -- "safe" | "review" | "danger"
  risk_notes    TEXT,                          -- JSON array of rule ids hit
  from_cache    INTEGER NOT NULL DEFAULT 0,    -- bool
  accepted      INTEGER,                       -- NULL unknown, 1 chosen, 0 rejected (from feedback)
  executed      INTEGER,                       -- NULL unknown; set only by a shell wrapper reporting back
  tags          TEXT                           -- JSON array (e.g. ["database","docker"])
);
CREATE INDEX idx_interactions_ts        ON interactions(ts DESC);
CREATE INDEX idx_interactions_project   ON interactions(project_id, ts DESC);
CREATE INDEX idx_interactions_cache     ON interactions(input_nl, project_id, cwd);

CREATE TABLE vocabulary (
  term          TEXT NOT NULL,                 -- normalized lowercase, e.g. "pg"
  expansion     TEXT NOT NULL,                 -- e.g. "postgres"
  weight        REAL NOT NULL DEFAULT 1.0,     -- grows with confirmations, decays with time
  source        TEXT NOT NULL,                 -- "taught" | "learned" | "imported"
  last_used_ts  INTEGER NOT NULL,
  use_count     INTEGER NOT NULL DEFAULT 0,
  PRIMARY KEY (term, expansion)
);

CREATE TABLE snippets (
  id            INTEGER PRIMARY KEY,
  name          TEXT NOT NULL UNIQUE,           -- "pg-up"
  command       TEXT NOT NULL,
  description   TEXT,
  created_ts    INTEGER NOT NULL,
  use_count     INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE feedback (
  id             INTEGER PRIMARY KEY,
  interaction_id INTEGER NOT NULL REFERENCES interactions(id) ON DELETE CASCADE,
  verdict        TEXT NOT NULL,                 -- "good" | "bad"
  note           TEXT,
  ts             INTEGER NOT NULL
);

CREATE TABLE shell_history (                    -- only populated if ingested
  id    INTEGER PRIMARY KEY,
  ts    INTEGER NOT NULL,
  cwd   TEXT,
  cmd   TEXT NOT NULL,                          -- redacted at ingest
  exit_code INTEGER,
  source TEXT NOT NULL                          -- "zsh" | "bash" | "fish"
);
CREATE INDEX idx_shell_history_ts ON shell_history(ts DESC);

CREATE TABLE meta (key TEXT PRIMARY KEY, value TEXT NOT NULL);
-- seeds: schema_version=1, created_ts, install_id (random uuid, local only)
```

Notes:

- **Everything is redacted at write time** (`input_nl`, `output_cmd`,
  `shell_history.cmd`). Storing a raw secret then stripping it at read time is one
  bug away from a leak; stripping on the way in is the only safe default. See
  [04-SAFETY.md](04-SAFETY.md) §3.
- **`executed` is nullable and honest.** The core never runs commands, so it
  cannot know. Only the optional shell wrapper (P2) reports back, via
  `shx feedback --executed <id>`.
- **No free-text embedding column in v1.** Embeddings are a v2 table, added by a
  later migration (ADR-008), so the v1 schema doesn't pay for them.

## 4. Retrieval: how context is chosen

Goal: give the model the *right* small slice, deterministically, inside a token
budget. This is a pure function with tests, not a model's judgment.

```
ContextBuilder::build(intent, env, store, budget) -> ContextBundle
```

Algorithm (v1 = "budgeted recency + relevance"):

1. **Anchor:** the last `recent = 10` interactions in `Scope::Project` (falls
   back to global if the project has fewer than 3), newest first.
2. **Relevance:** BM25-scored search of `Tool` history for tokens
   extracted from the intent (lowercased, stopword-filtered, plus any `vocabulary`
   term found), take top `relevance = 5` not already in the anchor set.
3. **Shell proximity (only if ingested):** the last `shell = 5` shell entries in
   the current cwd/repo, so the model knows what you were just doing.
4. **Vocabulary:** every `vocabulary` entry whose `term` appears in the intent,
   plus the top 5 by `weight` — the shorthand dictionary.
5. **Snippets:** user-named macros whose name/description matches the intent
   tokens (max 3). Managed via `shx snippet save|list|show|rm`. They are
   prompt context only — `shx <name>` is never a snippet lookup or an
   execution path (see [05-CLI-SPEC.md](05-CLI-SPEC.md) §3).
6. **Dedupe + order:** interleave newest-first, cap by token budget
   (`context.max_tokens`, default 1500, estimated as `chars/4`).
7. **Redact:** run the `Redactor` over every assembled block.

Output is a `ContextBundle` with a stable serialization order so prompts are
byte-comparable in golden tests.

**Budget is a hard cap.** If the anchor set alone exceeds it, entries are dropped
oldest-first from the tail, and `--why` reports the truncation. The model never
receives more than `max_tokens` of memory; the system prompt and profile are
counted separately.

**Alternatives considered** (see [07-DECISIONS.md](07-DECISIONS.md), ADR-005):
dumping the full last-100 window every time (rejected: waste + latency + dilutes
signal), and pure embeddings (rejected for v1: extra deps/model, and keyword+BM25
covers the shorthand case well). Embeddings (`fastembed`) are P2 and slot in
behind the same `ContextBuilder` by replacing step 2 — no interface change.

## 5. Learning

Two mechanisms, both conservative:

1. **Explicit:** `shx teach pg postgres` inserts/updates a `vocabulary` row with
   `source="taught"`, `weight=2.0`. Taught entries outrank learned ones.
2. **Implicit:** when a translation uses a term and the user later marks it `good`
   (`shx feedback`), the terms flagged in that interaction get `weight += 0.5`
   (capped at 3.0). Terms decay: `weight *= 0.98` per 30 idle days at prune time,
   so stale shorthand fades. A learned term is only *applied* once
   `weight ≥ 1.5`, to avoid one-off coincidences becoming permanent vocabulary.

**Fast-path cache** (F-25) sits on top: an interaction is cache-eligible when its
risk level is `safe`, `accepted = 1` (or unmarked but repeated 2+ times), and the
**context fingerprint** matches. The fingerprint is a hash of
`(normalized_intent, os, shell, cwd, project_id)` — a cached `docker run` for
`/proj/a` must not silently apply in `/proj/b`. Cache misses are the norm; hits
are the reward for repeating yourself.

## 6. Retention & prune

- `[memory] retention_days` (default 180). `shx history prune` applies it, plus
  decays vocabulary weights and drops `danger`-risk interactions' commands after
  30 days unless `--keep-danger` (they're the most likely to contain something
  sensitive, and the least useful to replay).
- `shx history export --json` / `--jsonl` for portability; `shx history purge`
  deletes the DB file contents (with confirmation).
- There is no telemetry and no upload path. The only network egress in the whole
  system is the LLM backend call, and it receives only the redacted
  `ContextBundle` + intent.

## 7. Failure behavior

Memory is a **soft dependency**. If the DB is missing/unwritable/locked:

- Translation proceeds with an empty `ContextBundle`.
- A one-line warning goes to **stderr** (never stdout).
- `shx doctor` reports the problem with the path and the fix.

The tool must never fail to answer because it couldn't read its own history.

## 8. Testing hooks (see [08-TESTING.md](08-TESTING.md))

- `MemoryStore` has an in-memory implementation (`InMemoryStore`) used by all
  pipeline tests — no temp files, no SQLite in unit tests.
- Migration tests use `tempfile` + a checked-in v0/v1 fixture DB.
- Retrieval tests assert **exact** `ContextBundle` contents for fixed fixtures
  (order, truncation, dedupe, budget), and that redaction ran.
- Property test: for any generated interaction set, `build()` never exceeds the
  token budget and never panics.
