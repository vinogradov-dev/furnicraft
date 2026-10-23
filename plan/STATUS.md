# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing, with the M4 panel-geometry refinement in flight
- Working on: contour-first panels, stage 2 — 2D offset spike is green; contour schema awaits confirmation
- Gate: `cargo xtask ci` — green

## Done

- `feat(geom): add line and arc contour offsets`
- `fix(param): place included banding inside the nominal frame`
- `docs(decisions): define the contour-first panel model`
- `feat(ui): add the outliner and panel card`
- `feat(units): parse millimetre text`
- `feat(app): add the sample material catalogue`
- `fix(engine): resolve panel geometry from the catalogue`
- `feat(engine): rename panels`
- `feat(param): build edge bands and expose panel diagnostics`
- `feat(engine): add catalogue-aware panel material commands`
- `feat(model): store panel nominal geometry and materials`
- `feat(model): add the material catalogue`
- `docs(decisions): define panel, material and banding model`
- `docs(decisions): add architecture decision records`
- `docs(vendor): add rules for vendored code`
- `docs: add the agent status file`
- `build(xtask): add the ci gate`

## Next

1. Confirm the stage-2 contour schema and the two path rules below, then implement corner features and bows as document data with commands and undo/redo.
2. Stage 2: build `N`/`C`/`O`, group segments into tangent-continuous banding paths, and wire the contour into `param` and the panel card.
3. Stage 3: union all through cutouts, support boundary notches and enforce the connected-core invariant.
4. Stage 4: banding on cutout edges and openings.
5. Resume M5: viewport picking, synchronized selection and viewport move/rotate.

## Open questions

- Corner-feature schema: one equal-leg 45° chamfer length or two independent setbacks, and one radius per corner? What ids address corners and bows (reuse `VertexId`/`EdgeId`)?
- Partial banding on a tangent path: if a radius joins two edges, does banding one edge apply to the whole path, or is an incompatible partial path rejected? The current ADR implies one path is one banding unit.
- Chamfer joint: the ADR says a separate path with a butt joint, while the v1 rule says the later band overlaps at a sharp corner. Which rule wins on a chamfer corner?
- Bow and radius tangency: a bow with a non-zero sagitta meets a tangent corner radius at an angle, so it becomes a sharp path end. Is that expected, or should the radius follow the bow tangent?
- Tangent tolerance: the spike uses `1e-6` radians and a `1e-6` mm connection gap; confirm these are the documented defaults.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions.

## Known debt

- Panels are still rectangles: no chamfers, radii, bows or contour edges.
- Banding is still a rectangle strip per source edge; there are no derived tangent paths and no banding on cutout edges.
- Cutouts outside the core and overlapping cutouts are still rejected; stage 3 replaces those v1 rules.
- The panel card does not edit cutouts yet.
- The outliner is the only way to select a panel; the viewport has no picking or selection highlight.
- Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks; rendering and diagnostics use the catalogue.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
