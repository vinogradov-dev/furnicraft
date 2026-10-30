### Answering the next step

This is the session contract for {{PROJECT}}: the plan files are the memory of the work, and a fresh
session continues from them instead of reconstructing the state.

A session starts by reading:

1. `plan/STATUS.md` — the `STATE:` line and its `Next` paragraph.
2. `plan/BACKLOG.md` — the `## Now` section.
3. `plan/SESSION.md` — only while `state=doing`.
4. The files the current task's `files:` hint names, and their direct callers.

It does not read the workspace at large, the conversation history, `git diff` or `git log`. The diff
is a fallback for one case only — `SESSION.md` is missing or contradicts `STATE` — and even then only
for the paths the task names.

A request for the next step is answered from the plan files alone, in this order. Stop at the first
case that applies.

1. **An interrupted session.** `state=doing`, `plan/SESSION.md` exists, `STATE: base` differs from
   `HEAD`, and `steps` still has an open item. Say so and offer to continue, quoting the note's
   `task:`, `now:`, `blocked:` and `run:`. Resume at `now:` instead of starting the task over.
2. **A task left in progress.** `state=idle` but `## Now` still holds a `[~]` task. Ask whether to
   continue it or put it back to `[ ]`; do not pick the next task silently.
3. **A contradiction.** A `[x]` without a commit hash, a `base` that is not an ancestor of `HEAD`, a
   `files:` path that does not exist, a `SESSION.md` without `goal`/`now`/`run`. Report what is
   broken and stop: an answer read from a broken plan is worse than none. The plan gate names every
   case.
4. **Only then** answer from `STATE` and `## Now`: the current task, its `files:`, and the one after
   it. Short.

Success looks like this: the answer needs no code read, no history and no diff, and an interrupted
session continues from `now:` instead of starting from zero.
