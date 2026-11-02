# Status

STATE: milestone=M7 phase=S task=S1 state=doing base=90ef57c updated=2026-11-02

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
  radius. A3 edits a working copy in the viewport and A6 applies it as one command; both commands
  validate the ring before the document changes, and a contour too detailed for the cap of one
  extrusion is refused instead of stored. D2 is the card: a contour panel shows its ring summary,
  the material-driven thickness, the material and one banding row per ring segment through
  `panel.ring_banding.set`/`clear`, keeps the `Edit contour` entry and offers no corners, bows or
  cutouts; a ring band with a missing material is a diagnostic like an edge or corner band. D3 lets a
  cutout outline be a contour ring: `CutoutShape::Contour` must stay strictly inside the core for a
  through cut and may touch its edge for a blind one, overlap is decided on the sampled ring rather
  than its bounds, `param` extrudes the ring as the tool, and the schema moves to 14.
- Defects live in `## Bugs` of `plan/BACKLOG.md` with an `X` id and the dashboard prints the section;
  taking a bug moves its line into `## Now` in work order. X1 (the concave cap flap) is fixed; X2
  records that a legal ring over 1024 sampled points cannot be extruded, and waits for the owner's
  choice in `plan/questions.md`.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate is its own crate: `state-contract` (L0, zero dependencies) parses and checks
  `STATUS`/`BACKLOG`/`SESSION`, ships templates, the agent section, the README and MIT/Apache-2.0
  texts. `xtask` is a consumer; `AGENTS.md` §1.1 points at its section instead of repeating it.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

S1 is the live task: split `crates/model/src/lib.rs` by responsibility without changing behaviour,
the public API or the serialized schema. A7 follows it: write the contour panel and the contour edit
mode down in `docs/domain/` and an ADR, and fold the D1–D3 cutout rules into
`docs/domain/panels.md`. B1 (fillet and chamfer on a contour corner) follows. X2 waits for a decision
on the cap limit (`plan/questions.md`). P1–P3 in `plan/BACKLOG.md` continue the gate itself, now
inside `state-contract`, starting with `status` printing the resume block.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
