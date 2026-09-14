# 00 — Vision

## The problem

Developers run dozens of commands they can't recall verbatim: start a Postgres
container, kill whatever holds a port, remove an Ollama model, prune Docker
volumes, tail the right journald unit, re-attach a tmux session. The knowledge
exists; the *recall* doesn't. So the workflow becomes:

> terminal → browser/chat app → search → read → copy → terminal → paste

Every step after the first is a context switch, and context switches are the
most expensive thing in a terminal workflow.

**Root cause:** not a knowledge gap. A memory-effort gap. Holding exact syntax
for infrequently used commands is a bad use of human working memory.

## The product in one sentence

`shx` converts a short natural-language intent into the exact shell command for
*this* machine, and prints it — never running it.

## Who it's for

**Primary (v1):** the author — a developer on macOS (Apple Silicon, 16 GB) who
already runs Ollama with `qwen3:14b`, works in Docker and dev-containers, and
lives in zsh. Every design decision optimizes for *this* person's latency budget
and habits first; generalization is a bonus, not the goal.

**Secondary:** developers on Linux and Windows who want the same thing but
without a cloud dependency for routine translations.

**Explicitly not for:** people who want an autonomous agent that runs commands
for you. That is a different, much larger, and much riskier product (see
Non-goals). `shx` is a *translator*, not an *executor*.

## Why now / why a personal build

Off-the-shelf tools (shell-gpt, aichat, Warp's AI, Copilot CLI) already do
language→command translation competently. Building anyway is justified only by
what those tools don't do:

1. **Habit-tailored context.** They don't know your ports, your shell, your
   Docker-over-bare-process preference, or your personal shorthand.
2. **Local-first privacy with a cloud escape hatch.** Most either assume a cloud
   key or assume local-only; the interesting design is the routing policy
   between them.
3. **Designed-in safety semantics.** A tool whose *default output mode is a
   warning-annotated, non-executing command* is a different product from a tool
   where `--execute` is one flag away.
4. **It ties into the author's existing AI-agent/dev-container workflow.**

## Non-goals (v1)

- **Executing commands.** No `exec`, ever, in the core binary. Not "off by
  default" — absent. (`--run` is an explicit v2 discussion item in
  [07-DECISIONS.md](07-DECISIONS.md), and even then only as a separate opt-in
  wrapper.)
- **Live shell-buffer rewriting.** No ZLE/readline widget in the core binary.
  Optional thin wrappers may ship later as separate scripts.
- **Being a general chat assistant.** `shx chat` exists to *refine a command*,
  not to be a second personality in your terminal.
- **Being an agent framework.** No tool-calling loop, no file editing, no
  multi-step autonomous execution. One intent in, one command out.
- **A GUI / TUI app.** Text in, text out.
- **A hosted service.** There is no `shx` server. Config, memory, and history
  are local files.

## Success criteria

v1 ships when all of these hold:

1. `shx "<intent>"` returns a *correct, runnable* command for the top ~50
   commands the author actually looks up, on first try, ≥80% of the time.
2. Median wall-clock latency for the local path is under ~2.5s end-to-end on the
   reference machine (M5 Air, `qwen3:14b`), with time-to-first-output under 1s
   where streaming exists.
3. Wrong-command risk is bounded by construction: the binary contains no code
   path that spawns a user-derived command, enforced by an automated test.
4. Memory measurably helps: after 20 interactions in a project, repeated
   shorthand resolves without re-explanation, and `--why` shows which stored
   entries informed the answer.
5. Every destructive command pattern in the classifier corpus is flagged, with
   zero false-negatives on the danger corpus and a false-positive rate low
   enough that warnings remain credible (<10% of "review"-level flags on the
   benign corpus).
6. A fresh contributor — human or agent — can go from clone to a merged PR
   without asking the maintainer a question, using `AGENTS.md` +
   `docs/agents/TASK-BOARD.md` alone.

## Design principles (used to arbitrate disagreements)

- **Simplest thing that ships.** Print-only beats buffer-fill. SQLite beats a
  bespoke store. One binary beats a daemon.
- **The human is the safety mechanism.** Never automate the step where intent is
  confirmed.
- **Local by default, cloud on purpose.** Any egress is a deliberate,
  configurable, redacted act.
- **Memory must be legible.** If the tool used stored context, `--why` must say
  which entries and why.
- **Deterministic where possible.** Prompt assembly, retrieval, and risk
  classification are pure functions with tests — not emergent model behavior.
- **Cross-platform is a build target, not an afterthought.** CI runs all three
  OSes from M0.
