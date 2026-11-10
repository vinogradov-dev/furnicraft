# Backlog

One entry per task, stable id in front; a long entry may wrap onto indented lines. `[ ]` open, `[~]`
in progress (exactly one, in `## Now`, matching `plan/SESSION.md`), `[x]` done and in `## Done` with
the commit that closed it.

The `files:` hint names where the work starts and is required on every unfinished task: each path
must exist, so name a directory for a file the task creates. The hint is a hint, not a boundary: read
those files and their callers, not the workspace.

An entry may carry labelled parts after its opening sentence, in this order. They are what
`cargo xtask task <id>` prints, so a reader never reconstructs them from prose:

```text
- `[ ]` **A3** what the user gets, in one sentence — rules: B1, B4, B8.
  Why: the observation that makes it a task, with the numbers.
  Shape: what moves where, in which file, and the size before -> after.
  Done: how a reader knows it landed.
  Commits: one, or one per step.
  `files: crates/model/src/lib.rs, crates/model/src/contour.rs`
```

Only `Rules`, `Why`, `Shape`, `Done` and `Commits` are fields; `files:` stays the hint, and the
sentence before the first label is the lead that a one-screen status block shows.

Order is the order of work. Renumbering is not allowed; a new task takes the next free id in its
phase. The first unfinished task in `## Now` is the one `STATE: task=` must name; `## Bugs` and
`## Next` are not part of that order.

## Now

- `[ ]` **{{TASK_ID}}** what the user gets, in one line — rules: B1.
  Why: why it is a task now, with the number that makes it one.
  Shape: the file the work starts in and what changes there.
  Done: the check that proves it landed.
  `files: src/lib.rs`

## Next

## Done
