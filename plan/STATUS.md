# Status

STATE: milestone=M7 phase=U task=U44 state=idle base=HEAD updated=2026-11-07

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of stable-id segments (ADR 0040,
  schema 13); `Path` is the open or closed chain that `to_contour` turns into a ring, and a primitive
  stores the parameters that built it (ADR 0054, schema 18) while its mesh is derived.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards
  (ADRs 0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps the shape the user saw (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus eight
  point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- The UI plan (U1–U26) is shipped: SVG icons, a work plane from the current ortho view, a projection
  toggle, and draw/pick/snap for a `Path` (ADRs 0047–0052).
- X3–X6 and U30–U37 are all fixed (ADRs 0056–0061): the projection button draws the view it is in, a
  creation lands at the scene root unless a container is armed, the armed tool draws pressed, the pole
  views look down their plane, a new drawing pivots on its own shape, a selected chain is a screen-space
  stroke, and every kind of contour edits through one draft.
- X1, X2 and X7 are fixed: a legal ring sampled past the 1024-vertex cap, so the chord follows the
  shape (ADR 0062); a ribbon tool is one measured cell with an elided caption (ADR 0067).
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  U27 rasterises each white master at `pt × pixels_per_point`, bakes the widget colour into the pixels
  and caches per `(uri, px, colour)`, at 20 pt tools and 16 pt inline; U28 fixes the tonal emphasis
  (ADR 0066 adds a 22 pt ribbon button). 47 placeholders are in `plan/debt.md`.
- U38–U42 landed the visual pass: `theme.rs` owns two palettes and the type scale (ADRs 0063, 0069);
  `header.rs` returns a `HeaderAction` (ADR 0064); U39 landed the ribbon (22 pt icons over captions,
  ADR 0066); U40 folded the side panels over one header row and moved the view controls into a
  translucent cluster at the viewport's bottom centre; U41 added the *Settings* modal, the atomic
  `settings.json` (ADR 0068) and *Home*. U47 added the `Ctrl+K` palette, and U42 divided the central
  area into one, two or four views: `view_layout.rs` owns the split tree, its clamped borders and each
  leaf's camera and preset, `ViewportRenderer` draws one target with a scissored pass and a camera slot
  per view, and the orientation widget gained a home button and an optional pitch/yaw/roll readout.
  U43 made the five display styles real: `ShadingStyle` is frame data in `crates/scene` (ADR 0070),
  `crates/mesh` caches one deduplicated edge list per mesh, and the renderer draws biased visible
  edges, always-tested hidden edges, wireframe edges alone and blended back-to-front translucent
  faces. U44–U48 queue the rest, and ADR 0065 fixes visibility and lock as session state.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files: `updated=` is a real date no older than `base`, `task=` names the first
  unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

U44 is next: a soft contact shadow under every object that touches the floor. The first version is a
flat ellipse or rounded rectangle derived from the world bound, drawn unlit on the ground plane with
a radial falloff — no shadow map, no extra pass, no texture — and a toggle in the display settings.
It is view data, a `SceneFrame` field next to `grid`; a projected shadow map is a later decision.

Order: U44 → U45 → U48 → U46 (icons last) → U49. Read the U44 entry, `crates/scene`'s `Grid`, the
`Bounds` in `scene_build.rs` and the line/face passes in `ViewportRenderer`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
