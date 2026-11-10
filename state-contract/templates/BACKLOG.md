# Backlog

One entry per task, stable id in front; a long entry may wrap onto indented lines. `[ ]` open, `[~]`
in progress (exactly one, in `## Now`, matching `plan/SESSION.md`), `[x]` done and in `## Done` with
the commit that closed it.

The `files:` hint names where the work starts and is required on every unfinished task: each path
must exist, so name a directory for a file the task creates. The hint is a hint, not a boundary: read
those files and their callers, not the workspace.

An entry may carry labelled parts after its opening sentence — `Rules`, `Why`, `Shape`, `Done`,
`Commits` — and the `files:` hint at the end. `cargo xtask task <id>` prints them one by one, so a
reader never reconstructs them from prose; the shape is in the crate README.

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
