# Status

Updated: 2026-10-27. Rewrite this file at the end of every task; never append a log.

## Where we are

- Milestone: M6 — Properties and classification. M5 editing is done: the scene tree, node editing,
  typed placement, viewport picking, the orientation widget, both gizmos, the stored pivot and
  multi-node placement as one undo entry. The model design for M6 is accepted: `Assembly` and
  `Group` become one `Container` with typed properties, a component is a marker rather than a node
  kind, and `FurnType` is a six-digit tree (`GG.TT.VV`) with `000000` unclassified
  (ADRs 0034–0035, `docs/domain/furntypes.md`).
- Working on: the schema v7 model — `FurnType`, the property bag with `Structured` JSON, one
  `Container`, system keys, the component marker and migration of version 6 files.
- Direction: properties and classification come before save/load, so the first project files carry
  the final shape. Parametric products are user prototype scripts over these properties, not
  built-in builders; scripts are deferred.
- Assistant: the architecture is recorded (ADRs 0028–0032, 0034) but not implemented. Faber is a
  client of the command registry, reads and queries the scene as data, and is disabled by default;
  it needs save/load and the property/type commands before it is useful. View state moves to the
  session (0029) before the agent crate exists.
- Gate: `cargo xtask ci` — green
- Branches: `dev` is the working branch for new tasks; `main` holds only verified work, merged with
  `--no-ff` after a green gate (AGENTS 6.2)

## Done

- `docs: define furniture types and node properties` (ADRs 0034–0035, `docs/domain/furntypes.md`)
- `docs: record multi-node placement` (ADR 0033)
- `feat(app): move and turn the whole selection with one command`
- `feat(engine): add the node.placement.set_many command`
- `feat(model): place a batch of nodes atomically`
- `docs: record order versions as numbered folders` (ADR 0032)
- `docs: record the assistant architecture` (ADRs 0028–0031, `AGENTS.md` §4.11, `PROJECT.md`)
- `docs: record the stored pivot and world-aligned rotate rings`
- `feat(app): rotate with world-aligned rings about the stored pivot`
- `feat(ui): edit the node pivot in the inspector`
- `feat(engine): add pivot commands`
- `feat(model): store and edit the manipulation pivot`
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

1. Schema v7 in `furnicraft-model`: `FurnType` and its registry, the property bag including
   `Structured` JSON, one `Container` with a component marker, system keys, queries by kind/type/
   property and the version 6 migration. Tests cover serde round-trip, old files, the ungroup rule
   and prefix matching.
2. Engine commands: property set/clear and batch edits, container create, convert-to-group, and
   select-by-query; each is one revision and one undo entry.
3. UI: the properties card, the type picker and tree, type badges and select-by-type.
4. Save and load, with the project store outside `engine`, then the order bundle and pinning.
5. Viewport multi-select (Ctrl+click) and snapping for the gizmos.

## Open questions

- Folder naming, version statuses and the draft-to-production transition in the order registry.
- What API the material and order programs expose locally (CLI, socket, HTTP), and whether they ship
  with FurniCraft or stay external.
- Where the workshop profile lives: project, application settings or the resource system.
- Whether reference retrieval (catalogues, norms, past jobs) is needed, and in what form.
- Voice input and output: which platform or service, if any.
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

- Selection and open containers still live in `ui::Editor` and the camera in the shell, although
  ADR 0029 places them in the session. They have not moved yet.
- Command specs expose only an id and a label; there is no parameter schema for a model, a CLI or
  generated documentation.
- History records one entry per command. A drag of a whole selection is one command, but a task made
  of several commands (a future assistant task, for example) cannot yet be grouped into one step.
- Faber is designed (ADRs 0028–0032) but not implemented; with no save/load, a background job has no
  project to open.
- Save and load do not exist; `apps/furnicraft` always starts from `sample_session()`. This is a
  deliberate deferral, not an oversight.
- Parametric cabinets do not exist yet: M6 provides classification and properties, and product
  prototypes arrive later as user scripts over those properties.
- A component has no dissolve operation yet: `node.ungroup` refuses it, and
  `node.component.convert_to_group` only clears the marker; dismantling a component is not
  implemented.
- The property catalogue is not designed: user properties are free-form and the inspector has no
  type editor for `Structured` values yet.
- Furniture types are not on the engine or UI path yet: schema v7 is a model change, and commands
  and the properties card follow.
- The gizmos do not snap: translate and rotate angles are free, and snapping is not implemented yet.
- The rotate rings are world-aligned only; a local-axis mode is not implemented.
- The pivot is edited only in the inspector; there is no viewport pivot-pick tool yet.
- Move and Rotate act on the top-most selected nodes; the viewport has no Ctrl+click multi-select
  yet, so a multi-selection comes from the scene tree.
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
