# Status

Updated: 2026-10-24. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing, with the M4 panel-geometry refinement in flight
- Working on: stage 3 — geometry edits are never blocked by banding or machining; a shaped panel
  keeps its cutouts and subtracts them in 3D. Per-segment banding and the shaped-core cutout union
  are next
- Gate: `cargo xtask ci` — green
- Branches: `dev` is the working branch for new tasks; `main` holds only verified work, merged with `--no-ff` after a green gate (AGENTS 6.2)

## Done

- `feat(model): let geometry edits ignore banding and machining`
- `feat(geom): trace region rings from rectangles`
- `feat(model): union through cutouts and reject a split core`
- `feat(geom): add rectangle union, subtraction and connectivity`
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

1. Stage 3: per-segment banding — assign edge banding to one edge, build one strip per contour
   segment and draw a corner band on a radius or chamfer segment.
2. Stage 3: extend the union, connectivity and ring-tracing rules to shaped cores.
3. Stage 4: banding on cutout edges and openings, using the same tangent-path and inclusion rules.
4. Resume M5: viewport picking, synchronized selection and viewport move/rotate.

## Open questions

- Per-segment banding raises a core question: an included band on one tangent segment must step the
  core at the joint, or one included spec must still offset the whole tangent path. Decide the
  physically correct rule before changing the core offsets.
- At a sharp joint between two included paths with different thicknesses, the model uses butt connectors and leaves the step as-is. Confirm whether later-band coverage should replace it.
- Stage 4 needs `Region` rings as tangent paths; the conversion from traced polylines to `Contour` segments is not designed yet.
- How a shaped core should carry a true notch on its curved outline is not designed; the shaped-core tracer must sample it without losing the band paths.
- Tangent tolerance remains `1e-6` radians and a `1e-6` mm connection gap.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions.

## Known debt

- Adding new machining to a shaped panel is still rejected until the contour-aware cutout union lands.
- A tangent radius can leave a path partially banded; the first record on the path drives the current strip until per-segment banding lands.
- A corner banding record on a point or radius corner is stored but not drawn until per-segment strips land.
- `regions_from_rects` is orthogonal-only: it normalizes rectangles and cannot trace arc boundaries; shaped cores need the contour tracer.
- Blind cutouts that overlap any other cutout or cross the core edge are still rejected until the 3D edge-pocket stage.
- Included offsets on a shaped path are checked only through the core extents; a hostile band thickness can still make the core contour self-intersect instead of returning a typed error. Stage 3's union layer should own the stronger check.
- At a sharp joint where an included path meets an unbanded or non-included path, the butt connector keeps the adjacent nominal endpoint. The core bounds then include that corner and the core contour can contain a zero-width slit even though the body and band volumes are correct; stage 3 should replace the connector with the offset intersection.
- `PanelGeometry` reports shaped core extents with a zero core origin; the body and cutout tools use nominal coordinates, but cutout validation on a shaped panel only sees the core bounding box.
- Path-wide banding uses the first edge id as the band part identity; a closed rounded path reports its first edge, not a path id.
- The panel card does not edit cutouts yet.
- The outliner is the only way to select a panel; the viewport has no picking or selection highlight.
- Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks for some checks; rendering and diagnostics use the catalogue.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- The app always loads `sample_session()`; there is no scene load/save path.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
