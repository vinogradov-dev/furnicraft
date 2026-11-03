# Status

STATE: milestone=M7 phase=G task=D4 state=idle base=a0a8e94 updated=2026-11-03

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`, and answer the next step from `STATE`
and the backlog's `Now` section — `cargo xtask status` prints both. `AGENTS.md` §1.1 points at the
contract that orders that answer when a session was interrupted or the plan is broken.

## Where we are

- M7 — objects and the scene file, after M6: `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035).
- The shape base comes before hardware, output and the assistant: flat contours with arcs, contour
  operations, a sketch on a plane, then two panel variants. The live shape is
  `Contour` = closed ring of directed line and arc `ContourSegment`s, each with a stable
  `ContourSegmentId` and a per-ring counter (ADR 0040, schema 13), and the object card, the extrusion
  and the file all read that one representation.
- S2 is complete: `crates/param/src/solid.rs` is the one domain-free plan — a base extrusion plus
  boolean steps, with a tool plan per step — and `crates/param/src/constructive.rs` is the analytic
  single-extrusion backend. Panels, bands, cutout tools and constructive objects compile to it, and
  `object.collapse` keeps the exact mesh the viewport showed (ADR 0041).
- S3, G1 and G2 are complete: `crates/param/src/profile.rs` is the one planar conversion — document
  ring to analytic contour to exact `geom::Region` to `kernel::Profile`, with one chord tolerance.
  `Region` keeps exact `Contour` rings, so area, bounds and containment read lines and arcs; only
  sampling is lossy. `geom::region` unions, intersects and subtracts regions by tracing the boundary
  of the set, so a rectangle and a curved ring take one path and the core trace subtracts a through
  ring exactly (ADR 0043).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- A panel outline is either the nominal rectangle or a contour ring (D1). A contour panel keeps the
  ring, bands per ring segment through `RingBanding`, and `panel.contour.create` copies a
  constructive object's contour with the ring moved to the panel zero corner; a rectangle panel never
  converts. The edit math (A5) reshapes a ring through `Contour::{move_corner, trim_segment,
  split_segment}`, keeps every untouched id and an arc's centre and radius; A3 edits a working copy
  and A6 applies one command, both validating the ring first and refusing one too detailed for the
  cap. D2 is the card: ring summary, material-driven thickness, material, one banding row per ring
  segment through `panel.ring_banding.set`/`clear`, `Edit contour`, no corners, bows or cutouts; a
  missing ring-band material is a diagnostic. D3 lets a cutout be a contour ring kept strictly
  inside the core (through) or touching its edge (blind), decided on the sampled ring; `param`
  extrudes it as the tool, schema 14. A7 records it in `docs/domain/` and ADR 0042.
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

D5–D11 follow D4 with typed machining, the operation registry and pipeline, card sections,
groove/milling and banding operations. S4 adds the revision-keyed cache; H1–H2 bring hardware and
mounts. B1–B2, C1, L1 and P1–P3 follow. X2 waits for a decision on the cap limit
(`plan/questions.md`).

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
