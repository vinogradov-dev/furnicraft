# Status

STATE: milestone=M7 phase=U task=U1 state=doing base=19565ff updated=2026-11-03

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`, and answer the next step from `STATE`
and the backlog's `Now` section — `cargo xtask status` prints both. `AGENTS.md` §1.1 points at the
contract that orders that answer when a session was interrupted or the plan is broken.

## Where we are

- M7 — objects and the scene file, after M6: `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of directed line and arc
  `ContourSegment`s with stable ids and a per-ring counter (ADR 0040, schema 13).
- S2 is complete: `crates/param/src/solid.rs` is the one domain-free plan — a base extrusion plus
  boolean steps, with a tool plan per step — and `crates/param/src/constructive.rs` is the analytic
  single-extrusion backend. Panels, bands, cutout tools and constructive objects compile to it, and
  `object.collapse` keeps the exact mesh the viewport showed (ADR 0041).
- S3, G1 and G2 are complete: `crates/param/src/profile.rs` is the one planar conversion — document
  ring to analytic contour to exact `geom::Region` to `kernel::Profile`, with one chord tolerance;
  `Region` keeps exact rings, and `geom::region` unions, intersects and subtracts them by tracing the
  boundary of the set (ADR 0043).
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
- The UI plan is approved (2026-11-03, tasks U1–U23): themed toolbars — Scene, Contour, Primitives,
  Bodies — with SVG icons rasterised at render size, no creation buttons in the scene tree, a flat
  contour stored as a document `Path` (open or closed, schema 17), a work plane taken from the current
  ortho view, and an ortho/perspective toggle.
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) is fixed, and X2 records that
  a legal ring over 1024 sampled points cannot be extruded, waiting in `plan/questions.md`.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate is its own crate: `state-contract` (L0, zero dependencies) parses and checks
  `STATUS`/`BACKLOG`/`SESSION`, ships templates, the agent section, the README and MIT/Apache-2.0
  texts. `xtask` is a consumer; `AGENTS.md` §1.1 points at its section instead of repeating it.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

U1 is next: the `assets/` tree with an empty SVG per icon, grouped by toolbar theme. U2 adds the
runtime SVG loader and the icon button, U4–U9 make a flat contour a document `Path` with its own
commands, U10–U17 add the shape constructors, the work plane, the ortho toggle and the drawing tools,
and U18–U20 replace the scene-tree creation buttons with the themed toolbars. U21–U23 follow with the
primitives and the extrude operation, then S4 and P1–P3. X2 waits for a decision on the cap limit
(`plan/questions.md`).

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
