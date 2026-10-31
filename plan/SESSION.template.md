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
resume: the first action a fresh session takes, as a command or a concrete edit
map: the plan/map.md row to start from, one anchor line (optional)
decisions: choices made during the task that a reader would otherwise re-litigate
```

Rules:

- `task:`, `goal:`, `now:` and `run:` must be non-empty — `cargo xtask plan` fails without them, and
  a note missing any of them leaves an interrupted session with nothing to resume from;
- `run:` reproduces the state you are in; `resume:` is what you would do next if this session died.
  They are not the same thing, and a `resume:` that needs a diff to interpret is not a `resume:`;
- update it after every meaningful step and after every green gate run, not once at the end;
- `steps` names outcomes, not files touched;
- `run` must be a command a fresh session can execute immediately;
- when the task is committed, mark it `[x]` in `plan/BACKLOG.md` with the commit hash, rewrite
  `plan/STATUS.md` (new `STATE` line and `Next`), and delete this file.
- `map:` is optional; when present, it names the `plan/map.md` row the next session starts from.
