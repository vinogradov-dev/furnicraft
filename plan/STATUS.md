# Status

Updated: 2026-10-23. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing, with the M4 panel-geometry refinement in flight
- Working on: stage 3 — panel machining now lives in the nominal panel frame (ADR 0011, schema v3);
  the through-cutout union layer is next
- Gate: `cargo xtask ci` — green

## Done

- `feat(model): move panel machining to the nominal panel frame`
- `feat: add tangent-path banding and shaped panel strips`
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

1. Stage 3: union all through cutouts into one region, support boundary notches and enforce the connected-core invariant.
2. Stage 4: banding on cutout edges and openings, using the same tangent-path and inclusion rules.
3. Resume M5: viewport picking, synchronized selection and viewport move/rotate.

## Open questions

- At a sharp joint between two included paths with different thicknesses, the model uses butt connectors and leaves the step as-is. Confirm whether later-band coverage should replace it.
- The 2D union/boolean layer for through cutouts is not chosen yet; ADR 0010 defers it to stage 3.
- The path representation for inner cutout loops is not designed yet; stage 4 needs it.
- Tangent tolerance remains `1e-6` radians and a `1e-6` mm connection gap.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions.

## Known debt

- Shaped panels support edge and chamfer-corner banding but still reject machining until stage 3.
- Included offsets on a shaped path are checked only through the core extents; a hostile band thickness can still make the core contour self-intersect instead of returning a typed error. Stage 3's union layer should own the stronger check.
- At a sharp joint where an included path meets an unbanded or non-included path, the butt connector keeps the adjacent nominal endpoint. The core bounds then include that corner and the core contour can contain a zero-width slit even though the body and band volumes are correct; stage 3 should replace the connector with the offset intersection.
- `PanelGeometry` reports shaped core extents with a zero core origin; the body is built directly from the core contour, while machining still needs a core-local frame.
- Path-wide banding uses the first edge id as the band part identity; a closed rounded path reports its first edge, not a path id.
- The panel card locks outline edits while any banding or machining exists; the model is path-aware, the UI is still coarse.
- Cutouts outside the core and overlapping cutouts are still rejected; stage 3 replaces those v1 rules.
- The panel card does not edit cutouts yet.
- The outliner is the only way to select a panel; the viewport has no picking or selection highlight.
- Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks for some checks; rendering and diagnostics use the catalogue.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
