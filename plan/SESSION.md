# Session

```text
task: P13
goal: the work order may be empty, and the plan says so instead of inventing a task

steps:
  [ ] the empty order is a legal state; closing P13 leaves `task=-`

now: the empty order is a legal state; closing P13 leaves `task=-`
blocked: nothing
run: cargo xtask status P13
resume: read plan/notes/P13.md
```
