# 05 — CLI Specification

Every behavior below is a build contract. Exit codes, stdout/stderr split, and
config keys are referenced by other docs and by the task board — treat them as
interfaces.

---

## 1. Output discipline (read this first)

| Stream | Contents |
| --- | --- |
| **stdout** | The command **only** (or the JSON object with `--json`). Nothing else, ever. Pipelines and `$(…)` depend on this. |
| **stderr** | Explanation, confidence, risk banners, `--why`, warnings, errors, escalation notices. |

This split is the reason `shx "…" | pbcopy`, `CMD=$(shx "…")`, and
`shx "…" --copy` are all safe. If a warning had gone to stdout it would corrupt
every script. Tested: [08-TESTING.md](08-TESTING.md), T-CLI-2.

## 2. Primary invocation

```
shx [OPTIONS] <INTENT>...
shx <SUBCOMMAND> [ARGS]
```

`<INTENT>...` is the natural-language text as one or more shell words; they are
joined with spaces. Quote it in practice: `shx "run pg on 7000"`.

### Options

| Flag | Meaning | Default |
| --- | --- | --- |
| `-n, --count <N>` | Return up to N candidates | `1` |
| `--json` | Structured output (single JSON object on stdout) | off |
| `--explain <CMD>` | Reverse mode: explain an existing command | — |
| `-i, --interactive` | Refine session: follow-up turns | off |
| `--why` | Show which memory entries / profile fields were used, and the routing trace | off |
| `--local` | Force the local backend | — |
| `--cloud` | Force the cloud backend (error if unconfigured) | — |
| `--offline` | Forbid all network; use mock/fixtures (demo & tests) | off |
| `--copy` | Copy the result to the clipboard (feature `clipboard`) | off |
| `--exit-on-risk` | Exit 3 if risk ≥ Review (for wrapper gating) | off |
| `--no-memory` | Ignore memory for this call (no read, no write) | off |
| `--no-color` | Disable ANSI on stderr | auto (tty) |
| `--profile <name>` | Use a named context profile | `default` |
| `--project <path>` | Override project scoping | git root of cwd |
| `-y, --yes` | Skip *tool-side* prompts (e.g. purge confirmation) — never execution | off |
| `-v, --verbose` | Include raw model output and full error chains on stderr | off |
| `-q, --quiet` | Suppress non-essential stderr (keeps risk banners) | off |
| `--config <path>` | Use an alternate config file | discovered |

### Examples

```console
$ shx "remove the ollama model llama3"
ollama rm llama3

$ shx "start postgres in docker on port 7000" -n 3
# stdout: 3 commands separated by a blank line?  NO —
#   with -n, stdout is one command per line, no decoration; explanations on stderr
docker run --name pg -e POSTGRES_PASSWORD=postgres -p 7000:5432 -d postgres:16
docker run --rm -p 7000:5432 -d postgres:16
docker compose run --service-ports db

$ shx "kill what's on 3000" --json | jq -r .commands[0].command
lsof -ti tcp:3000 | xargs kill -9

$ shx --explain "find . -name '*.log' -mtime +7 -delete"
Deletes .log files in this tree not modified in 7 days. Risk: REVIEW (irreversible delete).
```

With `-n`, candidates beyond what the backend returned are simply absent
(requesting 3 of 1 prints 1 line). `--copy` copies the first (top-ranked)
candidate to the system clipboard *in addition to* printing stdout: without
the `clipboard` build feature, or with no display server available, it
degrades to a stderr warning and exit 0 — stdout is never affected.

## 3. Subcommands

### `shx doctor [--redaction-test] [--json]`

Self-check. Verifies: config parse + validation, DB path writable + schema
version, backend reachability (`ollama` reachable? model present? cloud key env
set?), redaction self-test, print-only invariant self-report, and the effective
resolved config. Exit 0 if healthy, 4 if a backend is unusable, 1 otherwise.
This is the first thing a new user (and every agent) runs.

### `shx history`

```
shx history [--limit N] [--project|--global] [--risk <level>] [--grep <pat>] [--json|--jsonl]
shx history show <id>
shx history export [--json|--jsonl] [--out <file>]
shx history prune [--older-than <dur>] [--keep-danger]
shx history purge [--all]
```

`prune` applies retention ([03-MEMORY.md](03-MEMORY.md) §6). `purge` deletes
content and is the one place `-y/--yes` matters as a *tool* confirmation.

### `shx snippet`

```
shx snippet save <name> --command "<cmd>" [-d "<desc>"]
shx snippet list [--json]
shx snippet show <name> [--copy] [--json]
shx snippet rm <name>
```

Snippets are user-vetted macros stored in the local memory DB. Matching
snippets (name or description vs intent tokens, max 3) enter the
`ContextBundle` and appear under `snippets:` in `--why`.

`shx <name>` is **not** auto-resolved to a snippet in v1 (surprising
behavior) and is never executed: a bare invocation is always treated as
natural-language intent. Retrieve the command explicitly with
`shx snippet show <name> --copy` (command only on stdout, so `| pbcopy`
works). With the `clipboard` build feature `--copy` also attempts the
system clipboard; the command is always printed regardless.

`save` redacts `command` and `description` before insert. Re-saving the
same name updates the stored command. `rm` of a missing name is an error
(exit 1).

### `shx teach`

```
shx teach <term> <expansion>     # e.g. shx teach pg postgres
shx teach --forget <term>
shx teach --list [--json]
```

### `shx feedback`

```
shx feedback <id> good|bad [--note "<text>"] [--executed] [--accepted]
```

`--executed` is how the optional shell wrapper reports that the command was
actually run (the core can't know). Feeds learning ([03-MEMORY.md](03-MEMORY.md) §5).

### `shx config`

```
shx config path                 # print the resolved config file path(s)
shx config show [--json]        # effective merged config
shx config get <key>
shx config set <key> <value>
shx config edit                 # launch $EDITOR on the global config
shx config init [--force]       # write a commented default config
```

### `shx import-history`

```
shx import-history --shell zsh|bash|fish [--file <path>] [--limit N] [--dry-run]
```

Opt-in shell-history ingest (Scope::Shell). Redacts every line **before** insert
and prints a summary (`imported 480, redacted 12, skipped 4 malformed`).
`--dry-run` shows what *would* be imported. Never runs without the user asking.

### `shx completion <shell>` / `shx man`

Generated by clap; also shipped pre-generated in release archives so users don't
need the binary to install completions.

### `shx chat` / `shx -i`

Refine mode: `shx -i "run postgres in docker on 7000"` then follow-ups like
`also mount ./data` and `use the alpine image`. Same pipeline, a `session_id`
groups the turns, memory records the final command. Exit is still print-only.

## 4. Config file

Discovered in this order (later overrides earlier):

1. built-in defaults
2. `$XDG_CONFIG_HOME/shx/shx.toml` (`~/Library/Application Support/shx/shx.toml`
   on macOS, `%APPDATA%\shx\shx.toml` on Windows)
3. project `<git-root>/.shx.toml`
4. env `SHX_*` (e.g. `SHX_BACKEND__MODE=cloud`)
5. CLI flags

```toml
# ~/.config/shx/shx.toml  (written by `shx config init`, fully commented)

[backend]
mode = "local-first"                 # local | cloud | local-first
escalate_below_confidence = 0.6
local_slow_ms = 8000
complexity_skip_local = false        # ADR-009: measure before enabling

[backend.local]
kind = "ollama"
base_url = "http://127.0.0.1:11434"
model = "qwen3:14b"
keep_alive = "30m"
num_ctx = 4096
timeout_ms = 8000

[backend.cloud]
kind = "anthropic"                   # anthropic | openai-compat
model = "claude-sonnet-4-5"
api_key_env = "ANTHROPIC_API_KEY"    # we store the NAME, never the key
timeout_ms = 20000
# For openai-compat instead:
#   kind = "openai-compat"
#   base_url = "https://openrouter.ai/api/v1"
#   api_key_env = "OPENROUTER_API_KEY"

[memory]
enabled = true
path = ""                            # "" = OS default location
retention_days = 180
context = { recent = 10, relevance = 5, shell = 5, max_tokens = 1500 }
ingest_shell_history = false
redact_secrets = true                # do not set this to false

[safety]
warn_on_risk = true
exit_on_risk = false
refuse_multi_command_on_risk = true

[context]
os = "auto"                          # auto | macos | linux | windows
shell = "auto"
in_container = "auto"
prefer_docker = true
ports = [3000, 5432, 7000]
notes = ""                           # freeform, injected into PROFILE

[ui]
color = "auto"                       # auto | always | never
timing = false
update_check = "never"
candidates = 1

[snippets]
# remote = ""                        # P2: git repo of shared snippets
```

Unknown keys produce a **warning** (stderr), not an error, so forward/backward
compat is painless. Invalid values produce an error with the key path and the
accepted set.

## 5. Exit codes

| Code | Meaning |
| --- | --- |
| `0` | Success; command printed. |
| `1` | Generic error (unexpected internal failure). |
| `2` | Usage error (bad flags, `--cloud` unconfigured, invalid config). |
| `3` | Risk gate: a command was printed but risk ≥ Review **and** `--exit-on-risk`. |
| `4` | Backend unavailable / unusable (local down with no escalation, auth failure). |
| `5` | Memory problem only (translation still printed) — soft, `--json` carries `"memory": "degraded"`. |
| `6` | Refused: the intent is judged unsafe to translate (see [04-SAFETY.md](04-SAFETY.md) §1). |

Wrappers gate on `3` and `4`; scripts gate on `0`.

## 6. `--json` shape (stable, versioned)

```json
{
  "version": 1,
  "input": "kill what's on 3000",
  "commands": [
    { "command": "lsof -ti tcp:3000 | xargs kill -9",
      "explanation": "Finds the PID holding tcp/3000 and force-kills it.",
      "confidence": 0.81 }
  ],
  "risk": { "level": "review", "rules": ["proc.broad-kill"], "notes": ["uses kill -9"] },
  "backend": { "id": "ollama", "model": "qwen3:14b", "escalated_from": null },
  "memory": { "used": true, "entries": 4, "project_id": "ab12…", "from_cache": false },
  "latency_ms": 1840,
  "exit_reason": "ok"
}
```

The `version` field is bumped on any breaking change to this shape; consumers
should check it. This object is the contract for shell wrappers and the MCP mode
(P2).

## 7. Interaction & accessibility

- **Color:** ANSI only when stderr is a tty and `color = auto`; honors
  `NO_COLOR` and `CLICOLOR_FORCE`.
- **No interactive prompts** except `history purge` and `config init --force`
  (both skippable with `-y`). The common path never blocks.
- **Reading intent from stdin:** if stdin is not a tty and no intent argument is
  given, read the intent from stdin (one line). This makes
  `echo "run pg on 7000" | shx` work, and is how a shell widget can pipe a
  pre-filled prompt line to `shx`.
- **Locale:** UTF-8 assumed; no locale-dependent behavior in output.

## 8. Shell integration (see ADR-001 / F-54)

The core is print-only. Two optional, separately installed wrappers recover the
original "fill the prompt in place" UX without touching the Rust binary:

- **zsh:** a ZLE widget bound to a key that runs `shx` on the current buffer, gets
  the command back, checks the exit code (refusing to fill if `3` with
  `--exit-on-risk`), and replaces the buffer without executing.
- **bash:** a `bind -x` + `READLINE_LINE`/`READLINE_POINT` equivalent.

Both are ≤ 30-line scripts shipped under `contrib/shell/`, both documented with
the exact install line, and both strictly opt-in. They must never `eval` — they
only write to the edit buffer.
