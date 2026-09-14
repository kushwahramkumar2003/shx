# shx

**Natural language in. Shell command out. Nothing executed.**

`shx` is a small, cross-platform Rust CLI that turns an intent you'd say out loud
into the exact shell command you'd otherwise Google:

```console
$ shx run postgres in docker mapping 7000
docker run --name pg -e POSTGRES_PASSWORD=postgres -p 7000:5432 -d postgres:16

$ shx kill whatever is on port 3000
lsof -ti tcp:3000 | xargs kill -9
```

You press Enter. `shx` never does. That is the whole safety model: the terminal
is the confirmation step, so there is no confirmation UX to build and no way for
a confident-but-wrong model to `rm -rf` your machine.

## Why this exists

The friction is real and recurring: you need a command you don't have memorized,
so you leave the terminal, open a browser or a chat app, find it, copy it, come
back, paste it. That's a five-second task turned into a two-minute detour. The
problem isn't knowledge — it's not wanting to hold dozens of exact syntaxes in
working memory.

## What makes it different from `sgpt` / `aichat` / Copilot CLI

Those tools translate language → command too. The bet here is **context
injection tuned to your habits**: your OS and shell, the ports you actually use,
your preference for Docker over bare processes, your shorthand vocabulary — and
a persistent memory of the last N interactions so "run pg on 7000" resolves
correctly the second time without being re-explained.

## Design pillars

1. **Print-only.** No `exec`, no shell-buffer hooking in the core. Identical
   behavior on macOS, Linux, and Windows, in any shell.
2. **Local-first.** Ollama by default; cloud is an escalation path, not the
   default path. Your history never leaves the machine unless you configure a
   cloud backend *and* the egress redaction pass passes.
3. **Lightweight.** Single static binary, no runtime, no daemon. Cold start in
   milliseconds; a network call only when you ask for a translation.
4. **Memory with a purpose.** Recent interactions + learned shorthand + a
   project-scoped view of where you are, assembled into a token-budgeted prompt.
5. **Safety is a feature, not a disclaimer.** A pure-Rust risk classifier
   annotates destructive output before you see it.

## Status

Pre-alpha — M0 foundation. The workspace compiles and `cargo xtask ci` is the
local gate. Translation, memory, and backends are not implemented yet. See
[docs/09-ROADMAP.md](docs/09-ROADMAP.md) for milestones and
[docs/agents/TASK-BOARD.md](docs/agents/TASK-BOARD.md) for the work split.

## Documentation map

| Doc | What it covers |
| --- | --- |
| [docs/00-VISION.md](docs/00-VISION.md) | Problem, users, non-goals, success criteria |
| [docs/01-FEATURES.md](docs/01-FEATURES.md) | Feature catalog, competitive landscape, roadmap tiers |
| [docs/02-ARCHITECTURE.md](docs/02-ARCHITECTURE.md) | Workspace layout, traits, data flow, crate boundaries |
| [docs/03-MEMORY.md](docs/03-MEMORY.md) | Memory model, SQLite schema, retrieval, context assembly |
| [docs/04-SAFETY.md](docs/04-SAFETY.md) | Print-only invariant, risk classifier, redaction, privacy |
| [docs/05-CLI-SPEC.md](docs/05-CLI-SPEC.md) | Commands, flags, config file, exit codes, output formats |
| [docs/06-BACKENDS.md](docs/06-BACKENDS.md) | Backend trait, Ollama/Anthropic/OpenAI-compat, routing, fallback |
| [docs/07-DECISIONS.md](docs/07-DECISIONS.md) | ADRs resolving every open question in the original notes |
| [docs/08-TESTING.md](docs/08-TESTING.md) | Test tiers, fixtures, golden corpora, CI matrix |
| [docs/09-ROADMAP.md](docs/09-ROADMAP.md) | Milestones M0–M6 and post-v1 backlog |
| [AGENTS.md](AGENTS.md) | Entry point for autonomous coding agents |
| [docs/agents/AGENT-GUILD.md](docs/agents/AGENT-GUILD.md) | Roles, codeship cycle, handoff protocol |
| [docs/agents/TASK-BOARD.md](docs/agents/TASK-BOARD.md) | Per-task specs, dependencies, waves |
| [docs/agents/tasks.yaml](docs/agents/tasks.yaml) | Machine-readable task ledger |
| [CONTRIBUTING.md](CONTRIBUTING.md) | Human contributor guide |
| [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) | Community standards |

## Building

```sh
cargo xtask ci      # fmt + clippy + test + deny + audit + typos + doc + size
cargo build --release
cargo run -p shx -- --version
```

## License

Dual-licensed under MIT OR Apache-2.0, the Rust ecosystem norm. See
[LICENSE-MIT](LICENSE-MIT) / [LICENSE-APACHE](LICENSE-APACHE) (added in M0).
