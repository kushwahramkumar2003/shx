# Pull Request

## Task

- Task id: <!-- e.g. T-201 (see docs/agents/TASK-BOARD.md) -->
- Milestone: <!-- M0–M6 -->
- Role: <!-- Architect | Core | Backend | Memory | Safety | CLI/UX | QA | Docs/DevRel | Release -->

## What & why

<!-- One short paragraph: what changed and why. Link the issue/ADR. -->

## Evidence

<!-- The exact commands you ran and the result. This is the "done" proof. -->

```
$ cargo xtask ci
<result: green / which step failed>
```

Named tests from the task's acceptance criteria (list them):

- [ ] `test_name_1`
- [ ] `test_name_2`

Conditional evidence:

- [ ] Perf-touching → `cargo bench` delta attached
- [ ] Prompt/backend-touching → `tools/eval` delta attached
- [ ] Safety-touching → classifier/redaction corpus output pasted below

## Contracts

- [ ] No execution path added (ADR-002)
- [ ] stdout still carries only the command / `--json` object
- [ ] No secret committed (fixtures use obviously-fake values)
- [ ] No frozen interface changed — **or** an ADR link is provided below
- [ ] No new dependency, **or** `cargo deny check` passes and it's justified
- [ ] No `unwrap`/`expect`/`panic!` on user input

Interface changes / ADR: <!-- link or "none" -->

## Docs & changelog

- [ ] Spec updated if behavior changed (link the doc/section)
- [ ] `CHANGELOG.md` `[Unreleased]` line added for user-visible change
- [ ] ADR added if a decision changed

## Unblocks

<!-- Which tasks does completing this unblock? (engine output) -->
- 

## Notes / follow-ups

<!-- Known limitations, deferred work, anything a reviewer should know. -->
