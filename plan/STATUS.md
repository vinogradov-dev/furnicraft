# Status

Updated: 2026-10-27. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M5 — Editing. The scene tree, node editing, grouping, typed placement, viewport
  picking, the orientation widget, the translate gizmo and rotation editing are done. Rotation is
  edited as intrinsic XYZ Euler angles in the inspector and by a rotate gizmo in the viewport; both
  write the stored unit quaternion. Both gizmos use one pivot: the first corner of the node's local
  bounds, so a panel and a container move and turn about the same predictable point. Assemblies and groups are separate container kinds: a viewport
  click selects a closed container as one object, double-click enters one level and Escape leaves one
  level. The inspector groups its properties into collapsible sections; the fold state is egui view
  state only and every section starts open.
- Working on: the next task is multi-node placement, then the assembly dissolve operation.
- Direction: manual furniture design comes first. Parametric cabinets and project save/load are
  parked at the end of the roadmap; until save/load lands the scene lives only in the session.
- Gate: `cargo xtask ci` — green
- Branches: `dev` is the working branch for new tasks; `main` holds only verified work, merged with
  `--no-ff` after a green gate (AGENTS 6.2)

## Done

- `docs: record the shared manipulation pivot`
- `feat(app): pivot both gizmos at the first corner of the local bounds`
- `feat(model): expose the manipulation pivot in local and world space`
- `feat(ui): group inspector properties into collapsible sections`
- `docs: record the rotation representation and gizmo`
- `feat(app): rotate the selection with a rotate gizmo`
- `feat(scene): preview a world rotation`
- `feat(ui): edit rotation in the placement card`
- `feat(model): present placement rotation as XYZ Euler angles`
- `feat(units): add degrees for human-readable angles`
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

1. Multi-node placement: move the whole selection with one command and one undo entry.
2. Dissolve assembly: the separate operation that dismantles a component, once furniture types give
   it type-specific rules.

## Open questions

- Top and bottom views stop five degrees short of the pole to keep the up vector defined. Confirm
  that a near-plan view is enough until an orthographic projection mode exists.
- The orientation widget has no keyboard or menu equivalent; decide whether the six views deserve
  shortcuts before the list grows.
- The inspector shows the parent name, local X/Y/Z and intrinsic XYZ Euler angles; world coordinates
  are not shown yet. Confirm whether a read-only world readout earns its space.
- Around Y = ±90° the Euler fields are gimbal-locked: editing X or Z moves the other angle. Confirm
  whether the card should show a gimbal warning or switch to a quaternion readout there.
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
- The gizmos do not snap: translate and rotate angles are free, and snapping is not implemented yet.
- Move and Rotate each act on one selected node; a multi-selection has no combined placement command.
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
