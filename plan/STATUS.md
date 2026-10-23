# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing, with the M4 panel-geometry refinement in flight
- Working on: contour-first panels, stage 2b — corner features, bows, commands and the shaped body are green; tangent banding paths are next
- Gate: `cargo xtask ci` — green

## Done

- `feat(model): add contour corner features and bowed edges`
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

1. Add a separate `CornerBanding` record (`panel.corner_banding.set/clear`) for chamfer corners and resolve tangent banding paths.
2. Map edges to paths, enforce one compatible banding per path, and make edge-banding commands update or clear the whole path.
3. Implement mixed per-segment offsets and sharp-joint joins in `geom`; build `C`/`O` and one band strip per path with butt joints on chamfers and the v1 extension at 90° corners.
4. Wire shaped-panel banding into `param` and the card, then remove the stage-2a rejection.
5. Stage 3: union all through cutouts, support boundary notches and enforce the connected-core invariant.
6. Stage 4: banding on cutout edges and openings.
7. Resume M5: viewport picking, synchronized selection and viewport move/rotate.

## Open questions

- Confirm that banding one edge of a tangent path applies to the whole path and that an incompatible parameter on another edge rejects the command.
- Corner banding schema and its order relative to edge banding for a chamfer corner.
- Bow-to-radius tangency: a non-zero bow meets a tangent radius at an angle, so the joint is sharp by geometry; confirm this is intended.
- The 2D union/boolean layer for through cutouts is not chosen yet; ADR 0010 defers it to stage 3.
- Tangent tolerance remains `1e-6` radians and a `1e-6` mm connection gap.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions.

## Known debt

- Shaped panels currently reject banding and machining; the body and panel-card controls exist.
- Banding is still a rectangle strip per source edge; there are no derived tangent paths and no banding on cutout edges.
- Cutouts outside the core and overlapping cutouts are still rejected; stage 3 replaces those v1 rules.
- The panel card does not edit cutouts yet, and shaped-panel geometry is reported from contour bounds rather than a core origin.
- The outliner is the only way to select a panel; the viewport has no picking or selection highlight.
- Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks; rendering and diagnostics use the catalogue.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
