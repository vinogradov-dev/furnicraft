# Status

STATE: milestone=M7 phase=A task=A3 state=idle base=dda986c updated=2026-10-31

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
  `ContourSegmentId` and a per-ring counter (ADR 0040, schema 13), and the object card, the extrusion
  and the file all read that one representation.
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- A panel outline is either the nominal rectangle or a contour ring (D1). A contour panel keeps the
  ring as its outline, bands per ring segment through `RingBanding`, and `panel.contour.create` copies
  a constructive object's contour into a new panel with the ring moved to the panel zero corner; a
  rectangle panel never converts. Its body and band strips come from the ring in `param`. The edit
  math is in (A5): `Contour::{move_corner, trim_segment, split_segment}` reshape a ring, keep every
  untouched segment id and mint a split part from the ring counter; an arc keeps its centre and
  radius. The card does not edit the outline yet: A3 is the mode, A6 the apply command, D2 the card.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate is its own crate: `state-contract` (L0, zero dependencies) parses and checks
  `STATUS`/`BACKLOG`/`SESSION`, ships templates, the agent section, the README and MIT/Apache-2.0
  texts. `xtask` is a consumer; `AGENTS.md` §1.1 points at its section instead of repeating it.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

A3 is the first open task: the contour edit mode over the edit math — enter on a panel outline or an
object contour, edit a working copy in the viewport with handles and a preview, leave by applying one
command or by cancelling. A6 then turns the apply into one command, D2 rebuilds the contour panel card
around thickness, material and per-segment banding; D3 adds a cutout by an arbitrary contour. P1–P3 in
`plan/BACKLOG.md` continue the gate itself, now inside `state-contract`, starting with `status`
printing the resume block.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
