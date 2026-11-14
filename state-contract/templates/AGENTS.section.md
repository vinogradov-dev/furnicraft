### Answering the next step

This is the session contract for {{PROJECT}}: the plan files are the memory of the work, and a fresh
session continues from them instead of reconstructing the state.

A session starts by reading:

1. `plan/STATUS.md` — the `STATE:` line and its `Next` paragraph.
2. `plan/SESSION.md` — only while `state=doing`.
3. `{{TASK_COMMAND}}` — the task `STATE:` names, in full: its fields, its `files:` hint with the
   production and test lines and the last commit that touched each file, the `plan/map.md` rows that
   name those paths, the debt and questions the task touches, the ADRs it cites and
   `plan/notes/<ID>.md`, the research note a previous session left. `{{TASK_COMMAND}} <ID>` answers
   the same for any task. Prompt it instead of grepping `plan/*.md`, `docs/` or `git log`.
4. The files the report names, and their direct callers.

It does not read the workspace at large, the conversation history, `git diff` or `git log`. The diff
is a fallback for one case only — `SESSION.md` is missing or contradicts `STATE` — and even then only
for the paths the task names. Research a session had to do goes into `plan/notes/<ID>.md`
(`templates/NOTE.md`): the task report prints it, so the next session does not repeat the search.

A request for the next step is answered from the plan files alone, in this order. Stop at the first
case that applies.

1. **An interrupted session.** `state=doing`, `plan/SESSION.md` exists, `STATE: base` differs from
   `HEAD`, and `steps` still has an open item. Say so and offer to continue, quoting the note's
   `task:`, `now:`, `blocked:` and `run:`. Resume at `now:` instead of starting the task over.
2. **A task left in progress.** `state=idle` but `## Now` still holds a `[~]` task. Ask whether to
   continue it or put it back to `[ ]`; do not pick the next task silently.
3. **A contradiction.** A `[x]` without a commit hash, a `base` that is not an ancestor of `HEAD`, a
   `files:` path that does not exist, a `SESSION.md` without `goal`/`now`/`run`. Report what is
   broken and fix it with the tool before answering: the plan gate names every case, and
   `--repair` on a write lets a plan with several violations be brought back one verb at a time, so
   a broken plan is never a reason to edit `plan/` by hand. Stop only when the tool cannot do it.
4. **Only then** answer from `STATE` and `## Now`: the current task, its `files:`, and the one after
   it. Short.
5. **An empty order.** `task=-` and `## Next` says nothing is scheduled: say so and name what waits
   in `## Next` — do not invent a task, and do not take one into the order without being asked.
   `{{TASK_COMMAND}} take <id> --at top` puts one back.

Success looks like this: the answer needs no code read, no history and no diff, and an interrupted
session continues from `now:` instead of starting from zero.
