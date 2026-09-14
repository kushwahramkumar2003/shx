# Agent Guild — Roles, Codeship Cycle & Handoff Protocol

This is the operating manual for autonomous coding agents (and humans) building
`shx`. Read [AGENTS.md](../../AGENTS.md) first, then this.

The goal: **an agent can pick up any task in [TASK-BOARD.md](TASK-BOARD.md), build
it end-to-end, and hand it off without a human in the loop and without stepping
on another agent.** Every rule here exists to make that true.

---

## 1. Roles

An agent claims a role per task (a task names its role). Roles define *which
files you may touch* and *who you hand off to*.

| Role | Owns | Typical crates/files | Reviews |
| --- | --- | --- | --- |
| **Architect** | Interfaces, ADRs, schema, cross-crate concerns | `docs/07-DECISIONS.md`, `shx-core` traits | All interface changes |
| **Core** | Domain logic, pure functions | `crates/shx-core/*`, `shx-config/*` | Core PRs |
| **Backend** | Model integrations, routing | `crates/shx-llm/*` | Backend PRs |
| **Memory** | Store, migrations, retrieval | `crates/shx-memory/*` | Memory PRs |
| **Safety** | Classifier, redactor, corpora | `shx-core/src/{risk,redact}.rs`, `tests/corpus/*` | **All** safety-touching PRs |
| **CLI/UX** | Arg tree, rendering, subcommands | `crates/shx-cli/*` | CLI PRs |
| **QA** | Test tiers, doubles, eval harness | `tests/*`, `tools/eval/*` | Test coverage claims |
| **Docs/DevRel** | Docs, README, CHANGELOG, site | `docs/*`, `README.md` | Doc truthfulness |
| **Release** | CI, xtask, packaging | `.github/*`, `xtask/*`, `Cargo.toml` release cfg | Release PRs |

**One task = one role.** If a task needs two roles' files, it is scoped
incorrectly — split it.

## 2. Definition of Ready (DoR)

A task is claimable only when **all** hold. If any fails, the agent does **not**
start; it either fixes the blocker (if it owns the blocking task) or comments the
gap on the task and picks a different one.

- [ ] Status is `pending`, has no unresolved `blockedBy`, and no `owner`.
- [ ] Deliverable paths in the task don't overlap with another `in_progress` task.
- [ ] Every interface it consumes is `Accepted` (not `Proposed`) and exists in
      `main`.
- [ ] Acceptance criteria are testable (named tests or exact assertions).
- [ ] The relevant ADR / spec section is linked and readable.

## 3. The Codeship Cycle

The lifecycle of one task. Follow it in order. Do not skip steps — especially the
verify and handoff steps, which are where agents usually cheat.

```
   ┌────────────────────────────────────────────────────────────┐
   │ 1. ORIENT   read AGENTS.md, AGENT-GUILD.md, the task, its   │
   │             linked spec section + ADRs. Confirm DoR.        │
   ├────────────────────────────────────────────────────────────┤
   │ 2. CLAIM    task_update status=in_progress, owner=<agent>.  │
   │             If using git: branch feat/<task-id>-<slug>.     │
   ├────────────────────────────────────────────────────────────┤
   │ 3. BUILD    write code + tests together. Small commits.     │
   │             Stay inside your role's file ownership.         │
   ├────────────────────────────────────────────────────────────┤
   │ 4. VERIFY   cargo xtask ci  (fmt, clippy -D warnings, test,  │
   │             deny, audit, typos, doc, release build, size).  │
   │             Plus the task's own named tests. Green or stop. │
   ├────────────────────────────────────────────────────────────┤
   │ 5. DOCUMENT update the spec if behavior changed; add an ADR  │
   │             if a decision changed; add a CHANGELOG entry.   │
   ├────────────────────────────────────────────────────────────┤
   │ 6. HANDOFF  open PR (template), link PR in task metadata,   │
   │             task_update status=completed, then announce     │
   │             unblocked tasks and notify their owners.        │
   └────────────────────────────────────────────────────────────┘
```

### 3.1 ORIENT

Read, in this order: `AGENTS.md` → this file → the task in `tasks.yaml` → the
spec section the task cites → any ADR the task cites. Do not read the whole repo;
the task tells you what to read. If the spec contradicts the code, **the spec
wins** until an ADR says otherwise — file the discrepancy.

### 3.2 CLAIM

- Set the task `in_progress` and set yourself as `owner` **before** writing code.
  Claiming is the lock; it prevents two agents editing the same files.
- Branch name: `feat/<task-id>-<kebab-slug>` (e.g. `feat/t-201-memory-store`).
  Bugfix in a shipped area: `fix/<task-id>-<slug>`.
- Never work on `main` directly; never work in another task's branch.

### 3.3 BUILD

- **Tests ship with code.** A task isn't done because it compiles — it's done when
  its named tests pass. Write the test from the acceptance criterion first.
- **Stay in your lane.** Only edit files your role owns, plus the specific files
  the task names. Need to change someone else's file? That's an interface change
  (§4) or a new task.
- **Small commits**, imperative subject, optional conventional prefix:
  `feat(memory): add interactions table + migration v1`.
- **No `unwrap`/`expect` on user input**, no `println!` to stdout (stdout is the
  command channel — ADR-001 / [05-CLI-SPEC.md](../05-CLI-SPEC.md) §1).
- **Never** add an execution path (ADR-002). If a task seems to need one, stop and
  escalate to the Architect.

### 3.4 VERIFY

Run the same gate CI runs:

```sh
cargo xtask ci
```

This is non-negotiable and is the *only* acceptable evidence of "done". In
addition, run the task's named tests explicitly and paste their names in the PR.
If a test you didn't write fails, do **not** fix it by editing the test — that's
someone else's contract. Fix your code or, if the contract is genuinely wrong,
raise an interface change (§4).

Also required for certain tasks:

- Perf-touching: `cargo bench` delta.
- Prompt/backend-touching: `tools/eval` delta (T6).
- Safety-touching: classifier corpus output pasted in the PR.

### 3.5 DOCUMENT

- Behavior change → update the owning doc (`docs/`) **in the same PR**. A doc that
  lies is a bug.
- Decision change → new ADR in `docs/07-DECISIONS.md` (append; never edit a
  shipped decision — supersede it).
- Any user-visible change → a `CHANGELOG.md` `[Unreleased]` line.
- New CLI flag/subcommand → update [05-CLI-SPEC.md](../05-CLI-SPEC.md), including
  the exit-code table if it changed.

### 3.6 HANDOFF

Open a PR using `.github/PULL_REQUEST_TEMPLATE.md`, including:

- **Task id** and link.
- **What changed** and **why** (one short paragraph).
- **Evidence**: the exact commands run and their result (`cargo xtask ci` green,
  named tests listed).
- **Interface changes**: none, or the ADR link.
- **Docs touched** and **CHANGELOG** line.
- **Unblocked tasks**: which tasks this completion unblocks.

Then, on the task ledger:

1. `task_update status=completed`.
2. Write metadata: `branch`, `pr_url`, `verify=<evidence>`, `notes`.
3. **Announce what this unblocked.** The engine reports unblocked tasks on
   completion — for each, notify its owner (or leave it ownerless for the next
   agent) and mention it in the PR description. This is the "better flow" the
   guild exists for: completing work *visibly* frees the next work.

Then stop and report a 3-line summary: task id, status, next unblocked tasks.

## 4. Interface freeze & change protocol

The frozen interfaces are the three traits in
[02-ARCHITECTURE.md](../02-ARCHITECTURE.md) §4 (`Backend`, `MemoryStore`,
`Redactor`), the DB schema ([03-MEMORY.md](../03-MEMORY.md) §3), the config keys
([05-CLI-SPEC.md](../05-CLI-SPEC.md) §4), the exit codes (§5) and the `--json`
shape (§6).

To change any of them:

1. **Stop.** Don't edit the interface in your feature PR.
2. Write an ADR (status `Proposed`) describing the change and its blast radius.
3. Get Architect sign-off (or, human-in-the-loop, the maintainer).
4. If approved: apply the change as its **own** PR, bump the `--json` `version`
   and/or DB `user_version` as needed, and update every dependent doc.
5. Add `blockedBy` edges from dependent tasks to the interface task, then remove
   them as each is migrated.

**Never** change a frozen interface as a side effect of a feature. That is the
single fastest way to break parallel agents.

## 5. Conflict avoidance (multiple agents, one repo)

- **File ownership by role** (§1) is the primary lock. Two tasks may run in
  parallel iff their deliverable paths are disjoint.
- **Interface owners** are serialization points: tasks that all touch
  `shx-core/src/types.rs` must run in the same wave and are ordered, not parallel.
  The task board marks these with `serialize: true`.
- **Migration numbers** are allocated in the task board; never invent a number.
  One migration per task, appended.
- **Shared files** (`Cargo.toml`, `CHANGELOG.md`, `docs/07-DECISIONS.md`) get
  append-only edits and are the usual merge-conflict spots — keep changes minimal
  and rebase before handoff.
- If you hit a conflict you can't resolve inside your lane: **stop, don't force
  it.** Comment the collision on the task and pick another ready task.

## 6. Parallelism plan (waves)

Tasks are grouped into **waves**; everything in a wave can run concurrently.
A wave starts when its dependencies (previous waves) are `completed`.

- **Wave 0** — bootstrap (single agent; serialization point).
- **Wave 1+** — fan out by role. See [TASK-BOARD.md](TASK-BOARD.md) §Waves.
- The Architect keeps *one* slot free to review interfaces as waves land.

Recommended concurrent-agent budget: **3–4**, matched to the reference machine
(a 16 GB laptop compiling Rust + running a local LLM) and to review bandwidth.
More agents than that mostly produce merge churn on `Cargo.toml` and the shared
docs.

## 7. Escalate to a human when…

- A task requires an interface change and no Architect is available.
- A safety invariant (ADR-002, T-SAFE-3) appears to conflict with the task.
- A dependency is unreachable, a model download is needed, or a cloud key is
  missing and the task can't use `MockBackend`.
- You discover the spec is wrong in a way that changes multiple docs.
- Two agents' tasks genuinely need the same file and the board is wrong about
  disjointness.

Escalation = leave the task `in_progress`, add a `notes` entry explaining the
blocker and what you tried, and open a `needs-decision` issue. Do not guess on
interfaces or safety.

## 8. Definition of Done (DoD)

A task is `completed` only when **all** are true:

- [ ] Acceptance criteria met, with named tests passing.
- [ ] `cargo xtask ci` green locally (same as CI).
- [ ] No new `clippy` warnings; no new deps without a `cargo-deny` pass.
- [ ] Tests included for the new behavior (not deferred to "a follow-up").
- [ ] Docs updated if behavior/CLI/config/schema changed.
- [ ] CHANGELOG entry for user-visible change.
- [ ] ADR added if a decision changed.
- [ ] PR opened and linked in task metadata.
- [ ] Downstream `blockedBy` edges handled (unblocked tasks announced).
- [ ] No execution path added; no secret committed; no force push.

## 9. Anti-patterns (auto-reject in review)

- Editing or deleting a test to make a red suite green.
- Changing a frozen interface inside a feature PR.
- `#[allow(clippy::…)]` without a one-line justification.
- `unwrap()`/`expect()` outside tests, or `panic!` on user input.
- Printing anything but the command to **stdout**.
- Adding a dependency that pulls OpenSSL, a second HTTP stack, or an async runtime
  without an ADR (ADR-006).
- Committing `shx.toml` with an API key, or any real secret in a fixture.
- Marking a task `completed` while CI is red or a named test is missing.
- `git push --force` to a shared branch (allowed only on your own feature branch,
  and never after review has started).
