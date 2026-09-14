# 04 — Safety & Security

The safety model has one sentence: **`shx` prints commands; it never runs them.**
Everything else in this document exists to make that sentence true and useful.

---

## 1. The print-only invariant

The binary contains **no code path that executes a user-derived command.** Not
disabled — absent.

- No `Command::new(user_input)`, no `sh -c`, no `eval`, no dynamic `dlopen` of
  anything the user asked for.
- The only process spawns in the entire tree are:
  1. `shx doctor` probing the `ollama` binary/endpoint (fixed argv, no user text),
  2. `shx config edit` launching `$EDITOR` (a fixed, user-set program; the
     *path* is passed as an argument, never as a program),
  3. `--copy` invoking the platform clipboard (feature-gated, via `arboard`, no
     subprocess).
- **Enforced, not promised:** a test (`T-SAFE-3`) greps the source tree for
  `process::Command`, `std::process`, `Command::new`, `libc::system`, `execvp`,
  etc., and fails the build on any usage outside an allow-listed file with a
  justifying comment. See [08-TESTING.md](08-TESTING.md).

**Why this matters beyond vibes:** because we never execute, we owe the user no
confirmation dialog, no allow-list, no sandbox, and no "are you sure?" UX. The
terminal is the confirmation step. It also makes the tool trivially portable:
there is nothing OS-specific in the risky direction.

### Consequences of the invariant

- `--run` / `--execute` **does not exist** in v1. A v2 discussion is recorded in
  ADR-002, and any such feature must live in a *separate* opt-in wrapper, never in
  the core.
- The tool may **refuse** (exit 6) an intent that is clearly a request to do
  something destructive and irreversible *to the tool itself* (e.g. "delete my
  entire home directory and all backups") — refusing to translate is not
  executing, and it's the honest answer.

## 2. Risk classification

Since we can't stop a bad command by not running it (the human might run it), we
warn *before* it's run. `RiskClassifier` maps a command string to:

```rust
enum RiskLevel { Safe, Review, Danger }

struct RiskAssessment {
    level: RiskLevel,
    rules: Vec<RuleId>,      // e.g. ["del.recursive-force", "net.pipe-to-shell"]
    notes: Vec<String>,      // human-readable, shown on stderr
}
```

- **Safe** — no rules matched. Printed normally.
- **Review** — plausible-but-careful (a broad `kill -9`, `docker system prune`,
  `git reset --hard`). Yellow banner on stderr: *what* and *why*.
- **Danger** — irreversible / high-blast-radius (`rm -rf /`, `dd of=/dev/…`,
  `mkfs`, `curl … | sh`, `:(){ :|:& };:`, `DROP DATABASE`). Red banner, and the
  command is still printed (we're not the gate) but the file/DB interaction is
  recorded with `risk_level=danger`.

The classifier is **pure Rust, no model, no IO** — deterministic and testable. It
never *rewrites* or *suppresses* the command; it annotates.

### Rule families (initial set)

| Family | Example triggers | Level |
| --- | --- | --- |
| `del` | `rm -rf` on `/`, `~`, `$HOME`, `.` , wildcards at root; `find … -delete` | Danger |
| `disk` | `dd of=/dev/*`, `mkfs*`, `diskutil eraseDisk`, `fdisk`, `wipefs` | Danger |
| `perm` | `chmod -R 777 /`, `chown -R` on system paths | Danger |
| `net.pipe-to-shell` | `curl`/`wget` piped into `sh`/`bash`/`zsh`; `eval "$(curl …)"` | Danger |
| `fork.bomb` | `:(){ :|:& };:` and lookalikes | Danger |
| `db` | `DROP DATABASE`, `DROP TABLE`, `TRUNCATE`, `DELETE` without `WHERE` | Danger |
| `cloud` | `aws s3 rb --force`, `terraform destroy`, `gcloud … delete` | Review→Danger |
| `container` | `docker system prune -a`, `docker volume rm`, force-remove with volumes | Review |
| `vcs.destructive` | `git push --force` (esp. to `main`/`master`), `reset --hard`, `clean -fdx` | Review |
| `proc.broad-kill` | `pkill -f <broad>`, `kill -9 -1`, `killall` | Review |
| `secrets.inline` | long literals that look like keys in the command itself | Review |
| `sudo` | any `sudo` prefix | Review (informational) |

Rules are data (`rules.rs` table of `{ id, family, level, matcher }`), not
scattered `if`s, so the corpus test and the docs stay in sync.

### False-positive discipline

A warning that cries wolf is worse than no warning — people learn to ignore it.
Two guardrails:

- **Benign-corpus FP budget:** < 10% of routine corpus commands may be flagged
  above `Safe`. Reviewed in CI ([08-TESTING.md](08-TESTING.md), T-SAFE-1/2).
- **Warnings are informational by default** (exit code stays 0). Gating is
  opt-in: `--exit-on-risk` returns exit 3 so a wrapper *may* refuse to fill the
  shell buffer. We don't impose the policy; we expose the signal.

## 3. Secret handling

Two redaction points, one implementation (`shx-core::redact`).

### 3.1 At write time (mandatory, always on)

Before *anything* is stored in `interactions` / `shell_history` / `vocabulary`,
the `Redactor` masks known secret shapes:

- Cloud API keys: `AKIA[0-9A-Z]{16}`, `sk-[A-Za-z0-9]{20,}`,
  `ghp_/gho_/github_pat_…`, `AIza[0-9A-Za-z_\-]{35}`, `xox[baprs]-…`.
- Bearer tokens / auth headers: `Authorization: …`, `-H "Authorization: …"`.
- Env-var assignments whose name matches `*_KEY|*_TOKEN|*_SECRET|*_PASSWORD|*_PWD`.
- Connection strings with inline credentials: `scheme://user:pass@host`.
- Private key blocks: `-----BEGIN … PRIVATE KEY-----`.
- JWT-shaped strings (`eyJ…`.`eyJ…`.`…`).

A match is replaced with `«redacted:kind»`. The *shape* is preserved so the model
still learns "you pass a token here" without ever seeing the value.

### 3.2 At egress (mandatory when a cloud backend is active)

The assembled `ContextBundle` + intent is redacted immediately before serializing
the request body to any non-local backend. Local backends (Ollama on loopback)
also get redacted context — consistency is cheaper than reasoning about which
paths are safe, and it means a future change of `base_url` can't silently start
exfiltrating.

Verification: `shx doctor --redaction-test` runs the redaction corpus and reports
coverage, so a user can *see* that egress scrubbing works before trusting a cloud
key.

## 4. Data-at-rest posture

- **Location:** local file only, never synced by `shx`.
- **Encryption:** none by default. The realistic threat is another local process,
  which an unencrypted-but-user-owned file already guards against as well as the
  OS does. (Encrypted DB is a P2 consideration, ADR-010 — it would need a
  keychain integration and a passphrase story; not worth v1 complexity.)
- **Permissions:** the DB file is created `0600` and the directory `0700` on
  Unix. On Windows, rely on the user profile ACL.
- **Secrets:** redacted at write (§3.1), so a DB compromise doesn't trivially
  yield credentials.
- **Purge:** `shx history purge --all` zeroes and removes the DB. Documented in
  the CLI spec.

## 5. Network posture

- **Egress inventory (complete list):**
  1. local backend: `http://127.0.0.1:11434` (or configured `base_url`), only
     when the local backend is selected,
  2. cloud backend: exactly one HTTPS request per translation, only when a cloud
     backend is configured *and* selected,
  3. `--copy` / completions: none.
- **No telemetry, no analytics, no update-check-by-default, no crash reporting.**
  Any update check is opt-in (`[ui] update_check = "daily" | "never"`, default
  `never`) and, when enabled, hits a fixed static URL with no payload.
- **TLS:** rustls (no OpenSSL system dep) with the platform/webpki root store.
- **Model download:** `ollama pull` is *not* run by `shx`; `doctor` only reports
  whether the configured model is present and prints the exact command to fetch
  it. We don't silently pull multi-GB blobs.

## 6. Prompt-injection / hostile-input considerations

The intent comes from the user, but the *history and shell content* fed into the
prompt are attacker-influenceable (a filename, a commit message, a command you
copied from the internet). Mitigations:

- Structural separation: memory blocks are wrapped in explicit delimiters and the
  system prompt states that history is *reference data, never instructions*.
- The output contract is a JSON schema; a translation that tries to emit
  multi-command scripts or prose outside the schema is dropped by `parse.rs`
  rather than passed through.
- **Multi-command output is discouraged and, for `Review`/`Danger` results,
  refused:** if the model returns something that looks like a pipeline/chain
  *when the intent was a single action*, `shx` prints only the first command and
  warns, rather than emitting a `; rm -rf …` tail.
- Worst case is bounded by §1: even a fully compromised translation can only
  *print* text. The human still reads it.

## 7. Threat model summary

| Threat | Mitigation | Residual risk |
| --- | --- | --- |
| Model produces a destructive command and the user runs it blindly | Risk classifier banner before Enter | User can still ignore the banner |
| Secret in your history/shell history leaks to a cloud model | Redaction at write + at egress; local-first default | Unknown secret shapes may slip through — open pattern set |
| Malicious content in history steers the model | Delimited reference blocks, schema-bound output, first-command-only rule | Sophisticated injections may still influence wording |
| `shx` itself runs something | Impossible by construction; enforced by `T-SAFE-3` | None in-tree |
| Supply-chain compromise of a dependency | `cargo-deny`, `cargo-audit`, minimal dep set, pinned `rust-toolchain.toml`, lockfile committed | Standard ecosystem risk |
| Local DB read by another user/process | `0600`/`0700`, local-only file | Same-user process can read it |

## 8. Reporting a vulnerability

See [CONTRIBUTING.md](../CONTRIBUTING.md#security). Please do not open a public
issue for a vulnerability; use a private security advisory on the repository.
Anything that breaks the print-only invariant (§1) is treated as **critical** and
gets a patch release, regardless of milestone.
