# Status

STATE: milestone=M7 phase=U task=U40 state=idle base=49a4b8a updated=2026-11-07

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
- Cameras and lights are scene objects (ADR 0036); the frame takes the first authored sun plus up to
  eight point lights in watts (ADR 0039), with a fixed exposure and no tone mapper.
- The UI plan (U1–U26) is shipped: SVG icons rasterised at render size, no creation buttons in the
  scene tree, a work plane from the current ortho view, a projection toggle, and draw/pick/snap for a
  `Path` (ADRs 0047–0052).
- X3–X6 and U30–U37 are all fixed (ADRs 0056–0061): the projection button draws the view it is in, a
  creation lands at the scene root unless a container is armed, the armed tool draws pressed, the pole
  views look down their plane, a new drawing pivots on its own shape, a selected chain is a screen-space
  stroke, and every kind of contour edits through one draft.
- X1 (the concave cap flap) and X2 are fixed: a legal ring sampled past the 1024-vertex cap, so the
  chord now follows the shape (ADR 0062).
- The icon pipeline was audited against a neighbour CAD project (ADR 0055, `assets/icons/AUDIT.md`):
  U27 rasterises each white master at `pt × pixels_per_point`, bakes the widget colour into the pixels
  and caches per `(uri, px, colour)`, at 20 pt tools and 16 pt inline; U28 gives a passive face 12–18 %
  fill, the result 45–60 % under its outline, one axonometry, and ink inside `3..21` at 20–45 %
  coverage (ADR 0066 adds a 22 pt ribbon button). 37 placeholders are in `plan/debt.md`.
- U38 landed the visual pass: `theme.rs` owns the palette, the 4 pt step, the row heights and the type
  scale (ADR 0063); `header.rs` draws one row and returns a `HeaderAction` (ADR 0064); the view
  controls left the viewport's first row for a bottom status bar. U39 landed the ribbon: one row of
  tabs — *Furniture, Hardware, Model, Sketch, Scene, View* — over the tool groups of the active tab,
  each tool a 22 pt icon over a 10.5 pt caption (ADR 0066). U40–U48 queue the rest of the owner's
  design with a session note each, and ADR 0065 fixes visibility and lock as session state.
- Faber's architecture is recorded (ADRs 0028–0032) but not implemented. `state-contract` (L0) checks
  the three plan files: `updated=` is a real date no older than `base`, `task=` names the first
  unfinished task in `## Now` (P2–P3). Gate: `cargo xtask ci` — green, `dev` → `main` with `--no-ff`.

## Next

U40 is next: the side panels become windows that fold, and the viewport gets its floating cluster.
The scene tree and the inspector each get a header row (title, count chip, fold chevron); folding
leaves a 26 pt strip with the icon and the rotated title, and a click, a double-click on its edge or a
drag outward restores it. The tree keeps only the tree, and the view controls leave the status bar for
a translucent cluster anchored `CENTER_BOTTOM` over the viewport: orbit/pan/zoom, frame all/home, the
display styles, the layout entry and the three-slider display settings popover, which also carries the
projection-and-angles switch U42 draws under the orientation cube. The bar keeps the armed tool, its
gesture, the counts and the last error — and now also the creation chip U39 moved out of the ribbon.

Order after U40, fastest first: U41 → U47 (command palette) → U42 → U43 → U44 → U45 → U48 → U46 (icons
last, so no surface waits on artwork). Read `plan/SESSION.template.md`, ADRs 0063–0066, then
`apps/furnicraft/src/main.rs` (`scene_tree`, `panel_card`, `status_bar`) and `crates/ui/src/lib.rs`.

## Index

| File | Holds | Read when |
|---|---|---|
| `plan/BACKLOG.md` | one line per task, ids, order, file hints | always |
| `plan/SESSION.md` | the live task: steps, blocker, next command | only while `state=doing` |
| `plan/debt.md` | accepted gaps, tagged by area | a task touches that area |
| `plan/questions.md` | decisions waiting for the owner | a task touches the question |
| `plan/archive/done.md` | history up to schema 11 | never, `git log` replaces it |
