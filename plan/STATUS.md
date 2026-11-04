# Status

STATE: milestone=M7 phase=U task=X2 state=idle base=HEAD updated=2026-11-04

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the current task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken or interrupted plan.

## Where we are

- M7 — objects and the scene file, after M6: `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of directed line and arc
  `ContourSegment`s with stable ids and a per-ring counter (ADR 0040, schema 13); `Path` (U4) is the
  open or closed chain, `to_contour` (U5) keeps its ids, and U6 stores it as `Geometry::Path` at schema 17.
- S2, S3, G1 and G2 are complete: `crates/param/src/solid.rs` is the one domain-free plan — a base
  extrusion plus boolean steps — evaluated by the kernel or by the analytic single-extrusion backend
  (ADR 0041); `crates/param/src/profile.rs` is the one planar conversion, document ring to exact
  `geom::Region` to `kernel::Profile` with one chord tolerance, and `geom::region` traces region
  boundaries for union, difference and intersection.
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- A panel outline is the nominal rectangle or a contour ring (D1–D3): corner features and bows
  parameterise the rectangle, a stored ring keeps its segments and per-segment banding, and
  `panel.contour.create` copies an object ring to the panel zero. `Contour::{move_corner,
  trim_segment, split_segment}` edit a working copy that one command applies, keeping untouched ids
  (A3–A7, ADR 0042).
- D4–D7 are complete: a rectangle's corner features and bows are parameters of its outline and
  every outline — rectangle or stored ring — resolves to one `geom::Region`; machining is one ordered
  list of anchored records whose static kind table drives both the staged effects in `crates/param`
  and the card sections in `crates/ui/src/machining/` (ADRs 0044–0046, schema 16).
- The UI plan is approved (2026-11-03, U1–U23) and shipped: themed toolbars — Scene, Contour,
  Primitives, Bodies — with SVG icons rasterised at render size, no creation buttons in the scene
  tree, a flat contour stored as a document `Path` (schema 17), a work plane from the current ortho
  view and an ortho/perspective toggle. U24: one ink on a 24 grid, three families (ADR 0047).
- S4 is complete: `crates/mesh` owns `EvaluationCache`, one entry per node keyed by the derivation
  inputs the mesh was built from, so a rebuild reuses every node an edit did not touch; a pass that
  visits fewer nodes prunes theirs. The viewport and `object.collapse` share
  `evaluate_constructive`, so a collapsed object keeps the shape the user saw (ADR 0053).
- `assets/icons/**` is embedded in `crates/ui/src/icons.rs`; `icon_button` draws at 16 pt tinted and
  falls back to its label, and the zoom slider rasterises at the new pixel size. U7–U23 draw and pick
  a `Path`, toggle the projection, take the work plane from the standard views, snap, build a chain
  and commit it as one path object (ADRs 0048–0052). The strip is the only creator; every
  implemented tool is drawn (U26); the actions set and the unbuilt cone, revolve and sweep stay empty.
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) is fixed; X2 — a legal ring over
  1024 sampled points cannot be extruded — moved to `## Now`, its options in `plan/questions.md`.
  `cargo xtask status` prints the state line, the head commit, the note and what comes next.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. The plan gate is its own
  crate: `state-contract` (L0, no dependencies) parses and checks the three files, and ships the
  templates, the agent section and the license texts; `updated=` must be a real date no older than
  the `base` commit, and `task=` must name the first unfinished task in `## Now` (P2–P3). Gate:
  `cargo xtask ci` — green. Branches: `dev`, merged to `main` with `--no-ff` (AGENTS §6.2).

## Next

X2 is next: a legal ring must extrude whatever its sample count. The ring samples at 50 µm, one
extrusion cap is one document face and `MAX_FACE_VERTICES` bounds it at 1024, while a legal ring may
hold 256 segments — so a smooth ring of a few turns is refused. The three ways out — raise the cap,
split a cap into several faces, or sample large rings coarser — are an owner decision, so X2 starts
with the answer, then a regression test that failed before the fix.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
