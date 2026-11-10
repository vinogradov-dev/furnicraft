# {{TASK_ID}} — the note's subject in one line

Copy this file to `plan/notes/{{TASK_ID}}.md` when a task's research outgrows the session note, and
delete it if the research turns out to be one sentence — that sentence belongs in the entry.

A note is the working memory of one task: what a session measured, which file and line it read,
which road it ruled out and why. `cargo xtask task {{TASK_ID}}` prints it beside the entry, so the
next session reads it instead of grepping the repository. Keep it under 150 lines.

What a note is not:

- not a second plan — no status, no `files:` hint, no commit; those live in `plan/BACKLOG.md`;
- not a changelog — the commit and the entry say what landed;
- not a conversation — no "the user said"; the decision and its reason, in the third person.

The note outlives its task: "why we did not do X" stays useful after the work lands. A note named
after a task the backlog does not have is a plan violation, so a task that is removed takes its
research with it.

## What was measured

- the file and the line, and the number that made the task worth doing;
- what the current code does, in one sentence each.

## Decisions

- the choice, and the one-sentence reason a reader would otherwise re-litigate.

## Ruled out

- the road not taken, and the evidence that closed it.
