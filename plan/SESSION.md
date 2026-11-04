# Session

```text
task: P3
goal: the plan gate fails when `STATE: task=` names a task that is not the first unfinished one in `## Now`

steps:
  [ ] the rule in `analyze`, with tests for a skipped task and for a valid first task
  [ ] the rule written down: README checks table and the AGENTS.md §6 list
  [ ] `cargo xtask ci` green and the plan closed

now: writing `first_task_violation` in `state-contract/src/validate.rs`
blocked: nothing
run: cargo test -p state-contract
resume: add the rule and its tests to `state-contract/src/validate.rs`
decisions: the rule reads `## Now` only — `## Bugs` and `## Next` are not the work order — and compares
  against the first not-`[x]` task, so a stray `[x]` in `## Now` keeps its own error
```
