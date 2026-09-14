# AGENTS.md — Entry point for autonomous coding agents

You are building **shx**: a print-only, local-first, memory-backed Rust CLI that
turns natural language into shell commands. Read this file, then
[docs/agents/AGENT-GUILD.md](docs/agents/AGENT-GUILD.md), then claim a task from
[docs/agents/TASK-BOARD.md](docs/agents/TASK-BOARD.md).

## The one rule that outranks everything

**`shx` never executes a command.** It prints. There is no `--run`, no `exec`, no
shell-buffer hooking in the core. This is enforced by test `T-SAFE-3` and by
ADR-002. If a task seems to require execution, stop and escalate — do not add it.

## Start here (in order)

1. **This file** — the rules.
2. **[docs/agents/AGENT-GUILD.md](docs/agents/AGENT-GUILD.md)** — roles, the
   codeship cycle, DoR/DoD, interface freeze, handoff protocol.
3. **[docs/agents/TASK-BOARD.md](docs/agents/TASK-BOARD.md)** — pick a task whose
   deps are done and whose files don't collide with an in-progress task.
4. **The spec section your task cites** — the spec is the contract; code follows
   it, not the other way around.

## The spec (read the section your task links)

| Doc | Contract |
| --- | --- |
| [docs/00-VISION.md](docs/00-VISION.md) | What/why, non-goals, success criteria |
| [docs/02-ARCHITECTURE.md](docs/02-ARCHITECTURE.md) | Crates, frozen traits, pipeline |
| [docs/03-MEMORY.md](docs/03-MEMORY.md) | SQLite schema, retrieval, learning |
| [docs/04-SAFETY.md](docs/04-SAFETY.md) | Print-only, risk classifier, redaction |
| [docs/05-CLI-SPEC.md](docs/05-CLI-SPEC.md) | Commands, flags, config, exit codes, `--json` |
| [docs/06-BACKENDS.md](docs/06-BACKENDS.md) | Backend trait, routing, output contract |
| [docs/07-DECISIONS.md](docs/07-DECISIONS.md) | ADRs — the "why" behind every choice |
| [docs/08-TESTING.md](docs/08-TESTING.md) | Test tiers, corpora, CI gate |

## Hard rules (violations are auto-rejected)

- **stdout is the command channel.** Only the resolved command (or the `--json`
  object) goes to stdout. Everything human-facing goes to stderr. Never
  `println!` anything else.
- **No execution path.** No `process::Command` with user-derived input, no
  `sh -c`, no `eval`. (Allow-listed: `doctor`'s fixed probe, `config edit`'s
  `$EDITOR`.)
- **Redact before store and before egress.** Secrets are masked at write time and
  again before any backend call. Never commit a real secret, even in a fixture.
- **Frozen interfaces change only via ADR.** `Backend`, `MemoryStore`, `Redactor`,
  the DB schema, config keys, exit codes, and the `--json` shape. See
  [AGENT-GUILD.md §4](docs/agents/AGENT-GUILD.md).
- **Stay in your role's files.** One task = one role = one branch. Don't edit
  another task's files; don't edit a test to make it pass.
- **No new deps without `cargo-deny` passing.** No OpenSSL, no second HTTP stack,
  no async runtime (ADR-006) without an ADR.
- **No `unwrap`/`expect`/`panic!` on user input.** Errors are typed and
  actionable.

## Verify (the only acceptable evidence of "done")

```sh
cargo xtask ci        # fmt + clippy -D warnings + test + deny + audit + typos + doc + release build + size
```

Plus your task's named tests. Green locally = green in CI (same command). If a
test you didn't write fails, fix your code — not the test.

## When you finish a task

1. `cargo xtask ci` green + named tests pass.
2. Update the spec if behavior changed; add an ADR if a decision changed; add a
   CHANGELOG line for user-visible change.
3. Open a PR with the template; link it in the task metadata.
4. Mark the task `completed`; **announce which tasks you unblocked** so the next
   agent can pick them up.
5. Report a 3-line summary: task id, status, next unblocked tasks.

## When you're blocked

Leave the task `in_progress`, write a `notes` entry explaining the blocker and
what you tried, open a `needs-decision` issue, and pick a different ready task.
Never guess on interfaces or safety.

## Repo facts

- **Language:** Rust (stable, pinned in `rust-toolchain.toml`).
- **Layout:** workspace of `shx-core` (pure, no IO), `shx-llm`, `shx-memory`,
  `shx-config`, `shx-cli` (bin), `xtask`. Depends inward only.
- **DB:** SQLite via `rusqlite` (bundled), WAL, `user_version` migrations.
- **HTTP:** `ureq` (sync, rustls). No `tokio` in v1.
- **Backends:** Ollama (default), Anthropic, OpenAI-compatible, Mock (tests).
- **OSes:** macOS, Linux, Windows — CI runs all three from M0.
