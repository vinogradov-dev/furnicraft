# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing, with the M4 panel-geometry refinement in flight
- Working on: contour-first panels, stage 1 — nominal frame and core origin
- Gate: `cargo xtask ci` — green

## Done

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

1. Spike a 2D line/arc offset and boolean layer; decide whether it is ours, part of the kernel adapter or a pure-Rust crate.
2. Stage 2: contour segments for chamfers, radii and signed bows; tangent-continuous banding paths and arc offsets.
3. Stage 3: union all through cutouts, support boundary notches and enforce the connected-core invariant.
4. Stage 4: banding on cutout edges and openings.
5. Resume M5: viewport picking, synchronized selection and viewport move/rotate.

## Open questions

- Tangent tolerance and the exact criterion for joining adjacent contour segments into one banding path.
- How to keep per-segment banding parameters compatible when an edit merges paths, and how the card should edit a whole path.
- The 2D offset/boolean implementation choice; see “Staged implementation” in `docs/decisions/0009-contour-first-panel-model.md`.
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
