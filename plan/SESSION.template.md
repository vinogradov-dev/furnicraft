# Session

Copy this file to `plan/SESSION.md` when a task starts, fill it in, and delete it when the task is
committed. It exists only while `STATE: ... state=doing`. Keep it under 50 lines: it is a working note,
not a report.

```text
task: A3
goal: one sentence, the user-visible result

steps:
  [x] what is already finished in this task
  [ ] what is still open

now: the exact sub-step in progress, with the symbol or file
blocked: nothing | what it waits for
run: the command that shows the current state (test, clippy, a manual check)
decisions: choices made during the task that a reader would otherwise re-litigate
```

Rules:

- update it after every meaningful step and after every green gate run, not once at the end;
- `steps` names outcomes, not files touched;
- `run` must be a command a fresh session can execute immediately;
- when the task is committed, mark it `[x]` in `plan/BACKLOG.md` with the commit hash, rewrite
  `plan/STATUS.md` (new `STATE` line and `Next`), and delete this file.
