# 01 — Features

Two parts: **(A)** what the tool does, and **(B)** the research that shaped it —
competitive landscape, gaps, and the extra features worth pulling in.

Every feature is tagged with a tier:

- **P0** — v1 core. The tool is useless or unsafe without it.
- **P1** — v1 if time allows; the differentiator vs. off-the-shelf tools.
- **P2** — post-v1 backlog, designed-for but not built.

---

## A. Feature catalog

### A1. Core translation

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-01 | `shx "<intent>"` → prints one command | P0 | The whole product. |
| F-02 | Structured output contract (JSON: candidates, explanation, confidence, risk) | P0 | Not free-text parsing. See [06-BACKENDS.md](06-BACKENDS.md). |
| F-03 | OS + shell + cwd awareness injected into the prompt | P0 | macOS/Linux/Windows, zsh/bash/fish/PowerShell. |
| F-04 | Multiple candidates (`-n 3`) | P1 | For ambiguous intents. |
| F-05 | `--explain "<command>"` reverse mode | P1 | Existing command → plain-English explanation. Cheap: same backend, different prompt. |
| F-06 | Interactive refine (`shx -i`) — follow-ups: "also mount the data dir" | P1 | Multi-turn, but still one command out. |
| F-07 | `--json` machine output for wrappers/scripts | P1 | Enables shell plugins without parse hacks. |
| F-08 | Alias to a fixed command (`shx -a dk "run pg"`) | P1 | For people who want `dk` self-contained. |

### A2. Context awareness

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-10 | Config-injected environment profile (OS, shell, ports, docker-preference) | P0 | The "habits" bet. See [05-CLI-SPEC.md](05-CLI-SPEC.md) `[context]`. |
| F-11 | Project-scoped context keyed by git root | P1 | "run the test suite" resolves per-repo. Hash of repo root as `project_id`. |
| F-12 | Container/dev-environment detection | P1 | Are we inside a container? Which compose service? |
| F-13 | `--why` explainability: which memory entries were used | P1 | Legibility principle from [00-VISION.md](00-VISION.md). |
| F-14 | Optional shell-history ingest (`shx import-history`) | P2 | Off by default; privacy-heavy. |

### A3. Memory

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-20 | Persistent interaction history (SQLite) | P0 | Last-N retrievable; fuller history for `shx history`. |
| F-21 | Token-budgeted context assembly (recency + relevance) | P0 | Deterministic, testable. See [03-MEMORY.md](03-MEMORY.md). |
| F-22 | Learned shorthand vocabulary (`pg` → postgres) | P1 | Auto-learned + explicit `shx teach`. |
| F-23 | Named snippets (`shx snippet save/list`) | P1 | Turn a good translation into a reusable macro. |
| F-24 | Feedback loop (`shx feedback <id> good|bad`) | P1 | Feeds few-shot selection + future tuning. |
| F-25 | Fast-path cache: exact repeated intent → cached command, no model call | P1 | Big latency win for repeats; must invalidate on context change. |
| F-26 | Semantic retrieval via local embeddings | P2 | `fastembed`; only if keyword/BM25 proves insufficient. |
| F-27 | Retention + prune (`shx history prune --older-than 180d`) | P1 | Data hygiene; also a `[memory] retention_days` auto-prune. |

### A4. Safety

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-30 | Print-only invariant (no execution path in the binary) | P0 | Enforced by test, not by convention. |
| F-31 | Risk classifier on output (Safe / Review / Danger) | P0 | Pure Rust, corpus-tested. See [04-SAFETY.md](04-SAFETY.md). |
| F-32 | Egress redaction (secrets stripped before any cloud call) | P0 | Mandatory when a cloud backend is active. |
| F-33 | `--exit-on-risk` for wrapper gating | P1 | Exit code 3 so a ZLE widget can refuse to fill the buffer. |
| F-34 | Secret-aware context: never store raw secrets in memory | P0 | Redact at write time, not just at egress. |
| F-35 | `shx doctor` self-check incl. redaction self-test | P0 | |

### A5. Backends

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-40 | Ollama local backend | P0 | Reference machine: `qwen3:14b`. |
| F-41 | Anthropic backend (Claude) | P1 | Structured output via tool-use. |
| F-42 | OpenAI-compatible backend (configurable `base_url`) | P1 | Covers OpenRouter, LM Studio, vLLM, llama.cpp, OpenAI proper. |
| F-43 | Routing policy: local-first, cloud escalation on low confidence/timeout/error | P1 | See [06-BACKENDS.md](06-BACKENDS.md). |
| F-44 | `--local` / `--cloud` force flags | P1 | |
| F-45 | Mock backend for tests/CI and `--offline` demo | P0 | Deterministic fixtures; no network in unit tests. |

### A6. Interface / packaging

| # | Feature | Tier | Notes |
| --- | --- | --- | --- |
| F-50 | Layered config (defaults → global TOML → project TOML → env → flags) | P0 | |
| F-51 | `--copy` to clipboard | P1 | Optional feature flag (adds deps). |
| F-52 | Shell completions + man page | P1 | Generated; shipped in release archive. |
| F-53 | Static release binaries (macOS arm64/x86_64, Linux gnu/musl, Windows) | P1 | `cargo-dist`. |
| F-54 | Thin shell wrappers (ZLE widget / readline binding) | P2 | Separate scripts, opt-in; recovers the v1 in-place UX without touching the core. |
| F-55 | `shx config path|get|set|edit` + `shx init` | P1 | |
| F-56 | Update check (opt-out) | P2 | Never a phone-home by default. |

---

## B. Research: landscape, gaps, and what to steal

### B1. What's already out there

| Tool | Shape | Translation | Memory | Local model | Safety default | Tailoring |
| --- | --- | --- | --- | --- | --- | --- |
| **shell-gpt (`sgpt`)** | Python CLI | Yes, good | None real | Via `--model`/local endpoints | Prints, can execute | None |
| **aichat** | Rust CLI | Yes | Session files | Yes (Ollama) | Prints, can execute | Config roles |
| **Warp** | Full terminal (Rust) | Yes | Session-scoped | No (cloud) | Suggests into buffer | Some |
| **Copilot CLI** | `gh` extension | Yes (`??`) | None | No | Copy-then-run | Repo-aware |
| **`llm` (Simon Willison)** | Python CLI + plugins | Via plugins | Logs in SQLite | Via plugins | Prints | Plugins |
| **`plz` / `yai` / `cmd-ai`** | Small single-purpose | Yes | Minimal | Varies | Prints | None |
| **Claude Code / Codex / Gemini CLI** | Agentic coding CLIs | Yes, as a side feature | Big | No | Tool-permission prompts | Deep, but heavyweight |

Patterns worth noting:

- **Almost everyone prints and lets the human run it.** That validates the
  print-only bet — it's the ecosystem default for a reason.
- **`llm` logs every call to SQLite.** The closest existing precedent for
  persistent memory; it stores history but doesn't *use* it as prompt context
  much. The gap `shx` fills is consumption, not capture.
- **Local-model support is now table stakes** (aichat, `llm` plugins, `sgpt`
  via OpenAI-compatible endpoints). Choosing Ollama is unremarkable; the
  interesting part is the *routing policy* on top.
- **Nobody personalizes to habit.** Warp has some session awareness; Copilot CLI
  is repo-aware. No mainstream tool learns *your* shorthand and *your* ports.
- **Risk awareness is nearly absent.** Output is printed raw. Nobody annotates
  `rm -rf` / `curl | sh` / `docker system prune` before you press Enter.
- **Agentic CLIs are converging on terminal-first AI**, but they're heavyweight
  (daemon, MCP, tool loops) and their NL→command path is incidental. There's a
  gap for a *tiny, focused* translator.

### B2. Gaps → our differentiating features

1. **Habit context** (F-10, F-11, F-12) — no competitor does it. Cheap to build
   (config + prompt), visibly better answers.
2. **Memory that's actually consumed** (F-20–F-26) — capture is common, use is
   not. `--why` makes it trustworthy.
3. **Risk annotation before Enter** (F-31, F-33) — the differentiator nobody
   ships. Turns "AI printed something scary" into "AI flagged why it's scary."
4. **Routing policy** (F-43) — local speed for the 90% case, cloud quality for
   the ambiguous 10%, with redaction at the boundary.
5. **Fast-path cache** (F-25) — repeated intents cost zero tokens and ~0ms.
   Gets stronger the longer you use it, which compounds the memory advantage.

### B3. Features to explicitly *not* copy

- **`--execute` / auto-run.** Kills the entire safety story and the reason this
  tool can skip confirmation UX. Deferred to a v2 discussion with heavy caveats
  ([07-DECISIONS.md](07-DECISIONS.md), ADR-002).
- **Full agent loop / tool calling.** Different product. Out of scope forever.
- **Daemon / background indexer.** Breaks "lightweight, single binary."
- **Hosted sync of your history.** Contradicts local-first. (Team-shared
  *snippets* via a git repo is a P2 maybe; your interactions never sync.)
- **Its own terminal.** Warp's bet; we're a one-shot binary inside the terminal
  you already use.

### B4. Ideas surfaced by research, queued as P2

- **MCP server mode** (F-52-adjacent): expose `shx` as an MCP tool so agentic
  CLIs can call it for command translation. Cheap to add once the core is a
  library, and rides the 2026 MCP wave instead of fighting it.
- **`shx explain-offer`**: when a command is risk-flagged, auto-offer the
  plain-English explanation inline.
- **Cheatsheet synthesis**: periodically mine `vocabulary` + `interactions` into
  a personal cheatsheet (`shx cheatsheet --md`) — the memory store makes this
  nearly free, and it's a nice public artifact.
- **Team snippets repo**: `[snippets] remote = "git@…"` to share vetted macros
  without sharing history.
- **Terminal-bench-style eval harness**: a local benchmark of real intents →
  known-correct commands, to measure model/prompt changes objectively. This is
  arguably P1 for the "measurably better" success criterion (#1) — see
  [08-TESTING.md](08-TESTING.md).
