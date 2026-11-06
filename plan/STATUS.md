# Status

STATE: milestone=M7 phase=U task=U38 state=idle base=ad95254 updated=2026-11-06

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
- The owner reviewed the editor and queued seven defects and two changes ahead of the icon work
  (X3–X6, U30–U37): the projection icon, the creation parent, tool highlight, the pole views, a
  centre pivot for new drawings, a visible selection outline for line chains, and editing every kind
  of contour. X3, X4, X6, U30 and U31 are fixed: the projection button draws the view it is in, every
  creation tool lands at the scene root unless a container is armed, the strip draws the armed drawing
  tool or primitive pressed while the duplicate "Draw" row is gone, the top and bottom views look
  straight down their plane (ADR 0057), and *Add child* arms a group or an assembly so the next
  created nodes land inside it while the strip names the arm (ADR 0056). U32 is fixed too: a new
  drawing stores the pivot of its own shape — the bounds centre when it is closed, the first stored
  segment's start when it is open — and an extruded body takes the ring's centre (ADR 0058). X5 is
  fixed: a selected chain takes one explicit colour and the shell repaints it over the viewport as a
  thick stroke with a dark underlay, because a one-pixel `LineList` cannot be widened (ADR 0059).
  U33–U37 are fixed: `PathCorner` names an open chain's corner, `Path` edits through the ring helpers
  (ADR 0060), a `ContourDraft` reshapes a path through one `object.path.replace`, the gizmo paints an
  open chain open, a cutout ring keeps its anchor and `CutoutDepth` when applied, and
  `panel.outline.to_contour` bakes a rectangle and its banding in one revision (ADR 0061).
- Defects live in `## Bugs` with an `X` id: X1 (the concave cap flap) and X2 are fixed — a legal ring
  sampled past the 1024-vertex cap, so the chord now follows the shape (ADR 0062).
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  `Image::tint` made a `#000` master black in every theme, and the 18 drawn of 51, pure stroke, had no
  mass at 16 pt. U27 rasterises each white master at `pt × pixels_per_point`, bakes the widget colour
  into the pixels and caches per `(uri, px, colour)`, at 20 pt tools and 16 pt inline. U28: a passive
  face fills at 12–18 %, the result at 45–60 % under its outline, the volumetric icons share one
  axonometry (§9.1), and each keeps its ink inside `3..21` at 20–45 % coverage, read off the raster;
  33 placeholders are in `plan/debt.md`.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files: `updated=` is a real date no older than `base`, `task=` names the first
  unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

U38 is next and it is a visual pass, not new behaviour: one top bar, the tool strip over the viewport,
a status bar naming the armed gesture, and one spacing scale for the window. It needs a written plan
and the owner's approval before code; 33 placeholder icons wait in `plan/debt.md`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
