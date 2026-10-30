# Status

STATE: milestone=M7 phase=D task=D1 state=doing base=HEAD updated=2026-10-30

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`. Do not scan the workspace, and answer
the next step from `STATE` and the `Now` section of the backlog. `cargo xtask status` prints both;
`AGENTS.md` §1.1 points at the contract that orders that answer when a session was interrupted or the
plan is broken.

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
- A contour panel is created from a contour selected in the scene, never converted from a rectangle.
  Its banding is per ring segment; it has no bows, no corner features and no parametric cutouts, only
  a cutout by an arbitrary contour. The card does not edit its outline: a dedicated mode edits a
  working copy and applies one command on exit, and the card is otherwise read-only. The panel model
  (D1) comes first, then the edit math (A5), the mode (A3), the commands (A6) and the card (D2).
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate is its own crate: `state-contract` (L0, zero dependencies) parses and checks
  `STATUS`/`BACKLOG`/`SESSION`, ships templates, the agent section, the README and MIT/Apache-2.0
  texts. `xtask` is a consumer; `AGENTS.md` §1.1 points at its section instead of repeating it.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

D1 is the first open task: the panel's outline becomes a rectangle or a contour ring, banding follows
the ring, and the document schema moves to 13. A5 is the edit math over that ring and A3 the mode
that uses it; A6 turns the apply into a command, D2 removes the rectangle-only controls from the card,
D3 adds a contour cutout. P1–P3 in `plan/BACKLOG.md` continue the gate itself, now inside
`state-contract`, starting with `status` printing the resume block.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
