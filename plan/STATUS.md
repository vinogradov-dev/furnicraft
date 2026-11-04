# Status

STATE: milestone=M7 phase=U task=U17 state=idle base=b203763 updated=2026-11-04

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files named by the current task in `plan/BACKLOG.md`, and answer the next step from `STATE`
and the backlog's `Now` section — `cargo xtask status` prints both. `AGENTS.md` §1.1 points at the
contract that orders that answer when a session was interrupted or the plan is broken.

## Where we are

- M7 — objects and the scene file, after M6: `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of directed line and arc
  `ContourSegment`s with stable ids and a per-ring counter (ADR 0040, schema 13); `Path` (U4) is the
  open or closed chain, `to_contour` (U5) keeps its ids, and U6 stores it as `Geometry::Path` at schema 17.
- S2 is complete: `crates/param/src/solid.rs` is the one domain-free plan — a base extrusion plus
  boolean steps, with a tool plan per step — and `crates/param/src/constructive.rs` is the analytic
  single-extrusion backend. Panels, bands, cutout tools and constructive objects compile to it, and
  `object.collapse` keeps the exact mesh the viewport showed (ADR 0041).
- S3, G1 and G2 are complete: `crates/param/src/profile.rs` is the one planar conversion —
  document ring to exact `geom::Region` to `kernel::Profile` with one chord tolerance; `Region`
  keeps exact rings and `geom::region` traces their boundary for union, difference and intersection.
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
  ortho view, and an ortho/perspective toggle. U1 lays `assets/icons/` — one placeholder per icon and
  `assets/README.md` with the naming and SVG rules. U24 fixes the contract on top of it: one ink on a
  24 grid, three families (geometry, view, service), the service grammar in a new `actions` theme,
  and the guide read only while an icon is drawn or edited (ADR 0047).
- U2–U3 embed `assets/icons/**` in `crates/ui/src/icons.rs`; `icon_button` draws at 16 pt, tinted
  and falling back to its label while an asset is empty, and the viewport strip's 75–200 % slider
  sets `ctx.set_zoom_factor` so icons rasterise at the new pixel size; the Contour icons are drawn (U25).
- U7–U16 draw and pick a stored `Path`, store it through `object.path.create`/`.replace`, toggle the
  projection, take the work plane from the standard views, snap, draft a chain and build it with
  the five tools while the viewport previews it (ADRs 0048–0051).
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) is fixed, and X2 records that
  a legal ring over 1024 sampled points cannot be extruded, waiting in `plan/questions.md`.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented.
- The plan gate is its own crate: `state-contract` (L0, zero dependencies) parses and checks
  `STATUS`/`BACKLOG`/`SESSION` and ships the templates, the agent section and the license texts.
- Gate: `cargo xtask ci` — green. Branches: work on `dev`, merge to `main` with `--no-ff` after a
  green gate (AGENTS §6.2).

## Next

U17 is next: commit a drawn path — `object.path.create`, the work plane as its local XY, the new
node selected and the tool reset, in exactly one undo entry. U18–U20 replace the scene-tree create
buttons with toolbars. U21–U23, S4 and P1–P3 follow; X2 waits.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
