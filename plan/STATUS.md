# Status

STATE: milestone=M7 phase=U task=U39 state=idle base=da23435 updated=2026-11-06

Read this file first. While `state=doing`, read `plan/SESSION.md` next; otherwise skip it. Then read
only the files the current task names in `plan/BACKLOG.md`, and answer from `STATE` and `## Now` —
`cargo xtask status` prints both; `AGENTS.md` §1.1 orders a broken or interrupted plan.

## Where we are

- M7 — objects and the scene file, after M6. `Container` is the only container kind, every node
  carries typed properties, a component is a marker rather than a node kind, and `FurnType` is a
  six-digit tree (ADRs 0034–0035). `Contour` is a closed ring of stable-id line and arc segments
  (ADR 0040, schema 13); `Path` is the open or closed chain that `to_contour` turns into a ring, and
  a primitive stores the parameters that built it (ADR 0054, schema 18) while its mesh is derived.
  `object.primitive.replace` is one revision and `object.collapse` bakes either payload.
- S2, S3, G1, G2, D4–D7 and S4 are complete: `crates/param` compiles one domain-free solid plan and
  one planar conversion (ADRs 0041, 0043), every panel outline resolves to one `geom::Region`, and
  machining is one ordered list of anchored records driving staged effects and the cards
  (ADRs 0044–0046, schema 16). `crates/mesh` caches by derivation inputs, so a rebuild reuses every
  untouched node and a collapsed object keeps the shape the user saw (ADR 0053).
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- The UI plan (U1–U26) is shipped: themed toolbars — Scene, Contour, Primitives, Bodies — with SVG
  icons rasterised at render size, no creation buttons in the scene tree, a work plane from the
  current ortho view, a projection toggle, and draw/pick/snap for a `Path` (ADRs 0047–0052).
- The owner reviewed the editor and queued seven defects and two changes ahead of the icon work;
  X3–X6 and U30–U37 are all fixed (ADRs 0056–0061): the projection button draws the view it is in, a
  creation lands at the scene root unless a container is armed, the armed tool draws pressed, the pole
  views look down their plane, a new drawing pivots on its own shape, a selected chain is a screen-space
  stroke, and every kind of contour edits through one draft.
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) and X2 are fixed — a legal ring
  sampled past the 1024-vertex cap, so the chord now follows the shape (ADR 0062).
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  `Image::tint` made a `#000` master black in every theme, and the 18 drawn of 51, pure stroke, had no
  mass at 16 pt. U27 rasterises each white master at `pt × pixels_per_point`, bakes the widget colour
  into the pixels and caches per `(uri, px, colour)`, at 20 pt tools and 16 pt inline. U28: a passive
  face fills at 12–18 %, the result at 45–60 % under its outline, the volumetric icons share one
  axonometry (§9.1), and each keeps its ink inside `3..21` at 20–45 % coverage, read off the raster;
  33 placeholders are in `plan/debt.md`.
- U38 landed the visual pass: `crates/ui/src/theme.rs` owns the palette, the 4 pt spacing step, the row
  heights, the rounding and the type scale (ADR 0063); `header.rs` draws one row — mark, name, Home,
  File, Save, Undo, Redo, the open tab, the gear — and returns a `HeaderAction` instead of touching
  the document (ADR 0064); the view controls and the armed-tool text left the viewport's first row for
  a bottom status bar, and the window keeps its system frame. U39–U46 queue the rest of the owner's
  design with the notes a fresh session needs.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files: `updated=` is a real date no older than `base`, `task=` names the first
  unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

U39 is next: the ribbon replaces the strip that still lives inside the scene tree. U38 landed the
groundwork it needs — `crates/ui/src/theme.rs` owns every colour, spacing step and row height, the
header returns a `HeaderAction` instead of touching the document, and the status bar already owns the
armed-tool text, so the strip can lose it. The owner fixed the order as "fastest first" and the names
as *Furniture, Hardware, Model, Sketch, Scene, View*; a PNG mockup was declined as too expensive, so
the ribbon is judged on screen. Read `plan/SESSION.template.md` before starting, then ADR 0063 and
0064, then `crates/ui/src/toolbars.rs` and `Editor::show_toolbars`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
