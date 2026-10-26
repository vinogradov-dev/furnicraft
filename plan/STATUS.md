# Status

Updated: 2026-10-26. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing. The scene tree, node editing, grouping, typed placement, viewport
  picking, the orientation widget and the translate gizmo are done. Assemblies and groups are now
  separate container kinds: a viewport click selects a closed container as one object, double-click
  enters one level and Escape leaves one level.
- Working on: rotation. The next task is rotation fields in the inspector and a rotate gizmo, then
  multi-node placement.
- Direction: manual furniture design comes first. Parametric cabinets and project save/load are
  parked at the end of the roadmap; until save/load lands the scene lives only in the session.
- Gate: `cargo xtask ci` — green
- Branches: `dev` is the working branch for new tasks; `main` holds only verified work, merged with
  `--no-ff` after a green gate (AGENTS 6.2)

## Done

- `feat: split assemblies from groups and pick closed containers`
- `docs: record the translate gizmo`
- `feat(app): move the selection with a translate gizmo`
- `feat(scene): project, drag along an axis and preview a translation`
- `feat(model): resolve world placements and collect panels under a node`
- `docs: record the view axes and the orientation widget`
- `feat(app): add the corner orientation widget`
- `feat(app): rework orientation widget, viewport mouse controls and default camera/light`
- `docs: record CPU ray picking and the viewport selection`
- `feat(app): pick nodes in the viewport`
- `feat(ui): expose viewport selection to the editor`
- `feat(scene): ray-cast drawables and tint the selection`
- `docs: document local placement editing`
- `feat(ui): edit node placement in the inspector`
- `feat(model): expose placement translation as exact lengths`
- `docs: define the scene-tree authoring roadmap`
- `feat(ui): add the scene tree with groups and node actions`
- `feat(engine): add scene node commands`
- `feat(model): edit, group and ungroup scene nodes`
- `docs: record arc band joints`
- `feat(param): trim arc band joints against their neighbours`
- `feat(geom): intersect lines and arcs analytically`
- `feat(param): trim band joints in application order`
- `feat(model): band by contour segment in one order`
- `fix(ui): keep typed corner numbers until the edit ends`
- `fix(model): remove corner banding with its segment`
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
- `feat(engine): resolve panel geometry from the catalogue`
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

1. Rotation: fields in the inspector and a rotate gizmo, both preserving translation and each
   committing one placement command on release.
2. Multi-node placement: move the whole selection with one command and one undo entry.
3. Dissolve assembly: the separate operation that dismantles a component, once furniture types give
   it type-specific rules.

## Open questions

- Top and bottom views stop five degrees short of the pole to keep the up vector defined. Confirm
  that a near-plan view is enough until an orthographic projection mode exists.
- The orientation widget has no keyboard or menu equivalent; decide whether the six views deserve
  shortcuts before the list grows.
- The translate gizmo pivots a panel at its local origin and a container at the minimum corner of
  its descendants' world AABB. Confirm whether a container pivot at the bounding-box centre is worth
  the extra derivation once rotation handles arrive.
- The inspector shows the parent name and local X/Y/Z; world coordinates are not shown yet. Confirm
  whether a read-only world readout earns its space.
- Rotation representation is still open: decide whether the inspector exposes Euler angles or a
  quaternion and which one the rotate gizmo writes.
- Double-click enters one container level and Escape leaves it. Confirm whether the tree should also
  show and toggle that open state, or stay an always-expanded explicit path.
- Open containers are view state and reset when the session starts. Confirm that this is preferable
  to persisting them with the workspace.
- Orbit is now on MMB (Shift+MMB pan, Ctrl+MMB zoom), LMB is selection. Confirm the wheel should
  remain a zoom duplicate of Ctrl+MMB.
- An included band on one tangent segment steps the core at the joint with its neighbour. Confirm the
  step is acceptable in practice; if not, revisit before changing the core offsets.
- At a sharp joint between two included paths with different thicknesses, the model uses butt
  connectors and leaves the step as-is. Confirm whether later-band coverage should replace it.
- The future material database schema stays open; see `docs/domain/materials.md` › Open questions.

## Known debt

- Save and load do not exist; `apps/furnicraft` always starts from `sample_session()`. This is a
  deliberate deferral, not an oversight.
- Parametric cabinets (dimensions driving panel sizes and placements) are out of the near-term plan.
- Assemblies have no dissolve operation yet: `node.ungroup` only flattens groups, and dismantling a
  component needs the type-specific rules that arrive with M6.
- The viewport has picking, a selection highlight and a translate gizmo; rotation has no typed or
  direct editor yet, so a loaded placement keeps the rotation it arrived with.
- The gizmo preview rebuilds every drawable's world vertices on each pointer move. An incremental
  transform buffer per drawable can replace the full upload when a large cabinet makes it visible.
- The orientation widget is hidden in a viewport smaller than its own size plus margins; there is no
  menu fallback for the six views yet.
- The selection highlight re-uploads every mesh when the selection changes; an incremental colour
  update or a GPU id pass can replace it.
- Region rings are traced for rectangles only; a shaped core has no contour-aware cutout union, so
  adding machining to a shaped panel is still rejected.
- The core outline at a line-arc or arc-arc joint still joins per-segment offsets with a straight
  connector; only the band strips are intersected analytically.
- Band trimming relies on 3D booleans; when truck finds no solid the earlier band is rebuilt untrimmed instead of failing the build.
- A legacy file can still carry a corner banding on a point corner; the card does not show it and it is removed when the corner next becomes a segment.
- Blind cutouts that overlap any other cutout or cross the core edge are still rejected until the 3D edge-pocket stage.
- Included offsets on a shaped path are checked only through the core extents; a hostile band thickness can still make the core contour self-intersect instead of returning a typed error.
- `PanelGeometry` reports shaped core extents with a zero core origin; the body and cutout tools use nominal coordinates, but cutout validation on a shaped panel only sees the core bounding box.
- The panel card does not edit cutouts yet.
- Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks for some checks; rendering and diagnostics use the catalogue.
- `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not implemented.
- `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
