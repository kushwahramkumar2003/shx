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

## Install

Rust 1.95 or newer. From GitHub:

```sh
cargo install --git https://github.com/kushwahramkumar2003/shx shx
```

From a checkout of this repo:

```sh
cargo install --path crates/shx-cli --locked
```

On macOS, accept the Xcode license first if linking fails (`sudo xcodebuild -license`).

## Use

Ollama is the default backend. Point it at any local model, then ask:

```sh
shx config init
ollama pull qwen2.5-coder:7b
```

Edit the generated config and set `model` under `[backend.local]`:

- macOS: `~/Library/Application Support/shx/shx.toml`
- Linux: `$XDG_CONFIG_HOME/shx/shx.toml` or `~/.config/shx/shx.toml`
- Windows: `%APPDATA%\shx\shx.toml`

```sh
shx "kill whatever is on port 3000"
```

Stdout is the command and nothing else. Warnings stay on stderr. Thinking models may reason before they answer; that trace is not printed. `shx` does not run the command.

`shx --offline "run pg on 7000"` prints a fixture command and does not call a model.

A cloud or other OpenAI-compatible server is selected in the same config with `[backend] mode = "cloud"` and `[backend.cloud] kind = "openai-compat"`, plus `base_url`, `model`, and `api_key_env`. The key stays in the environment variable named there.

## License

Dual-licensed under MIT OR Apache-2.0. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE).
