# Status

STATE: milestone={{MILESTONE}} phase=A task={{TASK_ID}} state=idle base=HEAD updated={{DATE}}

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`. Do not scan the workspace: the next
step comes from `STATE` and the `## Now` section, and the plan gate prints both.

## Where we are

- One bullet per fact a fresh session needs: what is live, what is recorded, what is not built yet.

## Next

`{{TASK_ID}}` is the first open task. Name the files it starts from and the order of the work; three
sentences is enough.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one entry per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
