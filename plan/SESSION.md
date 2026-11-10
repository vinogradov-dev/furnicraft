# Session

```text
task: P5
goal: a session start and any task question answered by the tool, with nothing left to reason about

steps:
  [x] `interrupted` in the summary, `goal`/`resume` in the status block, dashboard stops deriving it
  [x] `RELATED` — the notes of other tasks that name this task's files
  [x] `closed:` for a done task, `next:` only in `## Now`, `+N` filtered entries
  [x] `cargo xtask task list --section/--status/--phase/--grep`
  [x] panic lints at the crate root, docs and the plan row
  [ ] cargo xtask ci and the commit

now: the gate and the commit of P5
blocked: nothing
run: cargo xtask ci
resume: `cargo xtask ci`, then commit P5 and close it in the plan
decisions: `related` uses the existing `touches` needles, so one predicate decides what a plan entry
  and what another task's note say about this task; `list` filters live in xtask (phase names and
  `## Bugs` are FurniCraft vocabulary) and the crate only offers the parsed tasks.
```
