---
description: "Review a FurniCraft interface change against the design rubric"
argument-hint: "[paths, symbols or a diff range; empty reviews the working tree]"
---
Review the interface change: ${@:-git diff of the working tree}.

Read `.pi/skills/ui-design/SKILL.md`, then
`.pi/skills/ui-design/references/checklist.md`, and answer the form line by line. Load
`references/human-factors.md` for any line about size, timing, effort or contrast, and
`references/surfaces.md` to find the owner of a surface.

Rules of this review:

- Read only. Report; do not edit, and do not fix what you find unless asked in a later turn.
- One line per item: `PASS`, `FAIL` or `n/a`, with evidence — a file and symbol, a measured number,
  or a test name. An item without evidence is unanswered, not passed.
- Check the diff and the files it touches, not the whole crate. A claim about behaviour you did not
  read is a guess; say so.
- Separate what code can prove from what only the eye can see. The eyes-only list at the end of the
  form is answered "not checked" unless the task recorded an offscreen render or a live run.
- A rule you cannot cite from the form, an ADR or `AGENTS.md` is not a rule: drop it.

Finish with the verdict line (`PASS`, or `FAIL (n items)`), the debt entries the change should add
under `[ui]`, and the shortest list of what still needs a human look.
