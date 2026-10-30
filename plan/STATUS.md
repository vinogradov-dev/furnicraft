# Status

STATE: milestone=M7 phase=A task=A3 state=idle base=HEAD updated=2026-10-30

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`. Do not scan the workspace, and answer
"what next" from `STATE` and the `Now` section of the backlog. `cargo xtask status` prints both;
`AGENTS.md` §1.1 orders that answer when a session was interrupted or the plan is broken.

## Where we are

- M7 — objects and the scene file, after M6: `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035).
- The shape base comes before hardware, output and the assistant: flat contours with arcs, contour
  operations, a sketch on a plane, then two panel variants. The live shape is
  `Contour` = closed ring of directed line and arc `ContourSegment`s, each with a stable
  `ContourSegmentId` and a per-ring counter (ADR 0040, schema 12), and the object card, the extrusion
  and the file all read that one representation.
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate now enforces its own contract: `task=` outside `## Now`, a missing or dead `files:`
  hint, a session note without `task`/`goal`/`now`/`run`, a `base` that does not lead to `HEAD`, a
  wrapped hint nobody could parse, or a file over its line budget all fail `cargo xtask plan`.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

A3 is the first open task: a ring with an arc still has no editor, and the card disables its corner
text box. A5 — the edit math over the segment ring — is the base for that editor, and A6 turns it into
`object.contour.*` commands. The plan tooling continues in `plan/BACKLOG.md` as P1–P3, starting with
`cargo xtask status` printing the resume block so an interrupted session needs no `git` call.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
