# Contributing to shx

Thanks for wanting to help. `shx` is small on purpose; keeping it small is a
feature we defend.

## TL;DR

```sh
git clone <repo> && cd shx
cargo xtask ci        # the full local gate — run it before every push
cargo run -p shx -- doctor
```

- **Read the spec first.** `docs/` is the contract; code follows it. If the spec
  is wrong, change the spec (with an ADR) — not just the code.
- **One task = one branch = one concern.** Small PRs get reviewed fast.
- **Tests ship with code.** A PR without tests for new behavior is incomplete.
- **Never** add an execution path, a secret, or a dependency that pulls OpenSSL.
  See the hard rules below.

## Setup

1. **Rust** — stable, pinned in `rust-toolchain.toml`. `rustup` respects it
   automatically; run `rustup update` if prompted.
2. **A local model (optional but recommended)** — `ollama pull qwen3:14b`, or any
   instruct model ≥ 7B. You don't need it: `--offline` and the `MockBackend` cover
   all CI and most development.
3. **Tools used by the gate** (installed by `cargo xtask ci` on first run, or
   manually): `cargo-deny`, `cargo-audit`, `typos`, `cargo-nextest` (optional).
4. Build: `cargo build --release && ./target/release/shx doctor`.

## The gate (what CI runs, what you run)

```sh
cargo xtask ci
```

That's `fmt --check`, `clippy -D warnings`, `test`, a `--no-default-features`
build+test, `cargo deny`, `cargo audit`, `typos`, `cargo doc --deny warnings`,
and a release build with a size check. Local == CI, so green here means green
there.

Targeted runs while developing:

```sh
cargo test -p shx-core          # pure logic, instant
cargo test --workspace          # everything
cargo run -p shx -- --offline "run pg on 7000"
SHX_LIVE=1 cargo test --test live -- --ignored   # needs a real model
```

## Hard rules (PRs violating these are closed, not debated)

1. **No execution.** `shx` prints; it never runs. No `--run`, no `Command::new`
   on user input, no `eval`. Enforced by `T-SAFE-3`.
2. **stdout is the command channel.** Only the command (or `--json`) on stdout;
   everything human-facing on stderr.
3. **Redact before store and before egress.** Never commit a real secret, even in
   a test fixture — use obviously fake values (`sk-TEST…`).
4. **Frozen interfaces change only via ADR.** `Backend`, `MemoryStore`,
   `Redactor`, the DB schema, config keys, exit codes, `--json` shape.
   See [docs/07-DECISIONS.md](docs/07-DECISIONS.md).
5. **No new dependency without justification.** No OpenSSL (use rustls), no second
   HTTP stack, no async runtime without an ADR. `cargo-deny` must pass.
6. **No `unwrap`/`expect` on user input; no `panic!`** outside tests.

## Development workflow

1. **Pick or open an issue.** For anything non-trivial, agree on the approach
   first — a comment on an issue is cheaper than a rejected PR. Agent tasks live
   in [docs/agents/TASK-BOARD.md](docs/agents/TASK-BOARD.md); humans can use the
   same board.
2. **Branch:** `feat/<slug>`, `fix/<slug>`, `docs/<slug>`, `chore/<slug>`.
3. **Write code + tests together.** New behavior needs a test that fails without
   your change.
4. **`cargo xtask ci`** until green.
5. **Update docs** in the same PR if behavior changed; add a `CHANGELOG.md`
   `[Unreleased]` line for user-visible changes.
6. **Open a PR** using the template. Fill in the evidence section — the exact
   commands you ran and what happened.
7. **Review:** a maintainer reviews; safety-touching changes need a second
   reviewer. Address feedback with new commits (no force-push after review
   starts).
8. **Merge:** squash-merge with a conventional title (`feat(memory): …`).

## Where to contribute

- **Good first issues** are labeled `good first issue` — mostly classifier rules,
  corpus entries, docs, and CLI rendering.
- **Safety work is the highest-value work.** New rule families and corpus lines
  are small, testable, and directly protect users. See
  [docs/04-SAFETY.md](docs/04-SAFETY.md).
- **Never** send a PR that adds execution, telemetry, or a phone-home. Those are
  rejected on principle (they're non-goals in [docs/00-VISION.md](docs/00-VISION.md)).

## Tests and corpora

- Unit tests live next to the code; integration tests in `tests/`.
- The **risk corpora** (`tests/corpus/risk_danger.txt`, `risk_benign.txt`) are
  specifications: adding a rule requires adding lines. Zero false-negatives on the
  danger corpus is a hard gate.
- The **redaction corpus** is bidirectional: secrets must mask, and
  non-secrets (UUIDs, git SHAs) must pass through untouched.
- Snapshot tests (`insta`): run `cargo insta review` and **read** the diff before
  accepting — a blind `accept` defeats the purpose.

Details: [docs/08-TESTING.md](docs/08-TESTING.md).

## Commit & PR conventions

- Imperative subject, ≤ 72 chars: `feat(safety): flag curl-piped-to-shell`.
- Conventional prefixes: `feat`, `fix`, `docs`, `test`, `refactor`, `chore`,
  `perf`.
- Reference the issue/task id in the body (`Task: T-301`).
- Keep PRs focused: one concern. Mixed PRs get split.

## Community

- Be kind. [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md) applies.
- Questions → GitHub Discussions; bugs → issues (use the templates).
- Decisions with lasting consequences → an ADR in `docs/07-DECISIONS.md`, so the
  *why* survives past the PR.

## Security

**Do not open a public issue for a vulnerability.** Use a private security
advisory on the repository. Anything that breaks the print-only invariant is
treated as **critical** and gets a patch release. See
[docs/04-SAFETY.md](docs/04-SAFETY.md) §8.

## License

Contributions are dual-licensed under MIT OR Apache-2.0, matching the project.
By submitting a PR you agree to license your contribution under those terms.
