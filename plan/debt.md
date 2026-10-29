# Known debt

Gaps accepted for now. Read the entries whose tag matches the area of the task in hand; the whole file
is not part of a session start. An entry leaves this file when the fix lands.

Tags: `[view-state]` session versus document, `[render]` lighting and drawing, `[mesh]` geometry
editing, `[object]` constructive objects, `[panel]` panels and banding, `[scene]` tree and files,
`[gizmo]` manipulation, `[types]` properties and furniture types, `[engine]` commands and history,
`[perf]` frame and picking cost, `[assistant]`, `[vendor]`.

- `[view-state]` Selection and open containers still live in `ui::Editor` and the view camera in the
  shell, although ADR 0029 places them in the session. Cameras and lights are document nodes with
  viewport icons.
- `[render]` The frame uses the first authored sun light plus up to eight point lights; a ninth and
  later point light is stored but does not shade the viewport, and there is no culling by importance
  or distance. Point brightness uses a fixed shader exposure rather than a scene exposure, so a close
  or very powerful lamp clips its highlights; there is no tone mapper and there are no shadows.
- `[render]` An authored camera uses the scene default near and far planes because the model does not
  store them.
- `[render]` An authored camera pins the view and disables navigation, so the only way back to the
  navigable view is clearing the active camera in the inspector; a viewport-level "leave camera view"
  control is not implemented.
- `[object]` A constructive object supports one `Extrude` feature. The contour is edited as `X, Y` text
  lines in the object card, not in the viewport, and a ring that holds an arc has no editor at all
  until the sketch phase; there are no booleans, fillets or other feature kinds yet. Collapse is a
  one-way door and only undo brings the history back.
- `[object]` A contour's self-intersection test samples each segment into chords; a crossing finer than
  0.1 mm inside an arc can slip through. An exact segment-pair test can replace it when the sketch
  editor needs the last tenth of a millimetre.
- `[mesh]` Vertex markers and face fills are painted over the viewport image without an occlusion test,
  so a vertex on the far side of the mesh is visible and selectable through it. A depth readback or a
  pick pass can hide the back vertices when the first large mesh makes it matter.
- `[mesh]` Mesh editing selects and moves vertices and whole faces; there is no edge selection and no
  face extrusion yet.
- `[mesh]` A cube is created at the parent origin and centred on it, so half of a default cube sits
  below the floor until it is moved.
- `[perf]` Vertex picking and marker painting walk every vertex of the edited mesh each frame; a large
  mesh needs a spatial index or a GPU id pass. Face picking walks every polygon and its fan triangles
  each click.
- `[perf]` The gizmo preview rebuilds every drawable's world vertices on each pointer move. Camera and
  light drags already skip the upload, but a geometry drag still does not. An incremental transform
  buffer per drawable can replace the full upload when a large cabinet makes it visible. A vertex drag
  also re-tessellates the edited mesh on every pointer move instead of moving only the affected
  triangle vertices.
- `[perf]` The selection highlight re-uploads every mesh when the selection changes; an incremental
  colour update or a GPU id pass can replace it.
- `[perf]` `crates/mesh` still tessellates on every rebuild; the revision-keyed cache is not
  implemented.
- `[engine]` Command specs expose only an id and a label; there is no parameter schema for a model, a
  CLI or generated documentation.
- `[engine]` History records one entry per command. A drag of a whole selection is one command, but a
  task made of several commands (a future assistant task, for example) cannot yet be grouped into one
  step.
- `[assistant]` Faber is designed (ADRs 0028–0032) but not implemented; the scene store now gives a
  background job a file to open, but there is no headless binary or query surface yet.
- `[scene]` Save and load exist in the shell. Gaps: there is no prompt when the window closes with
  unsaved changes, Open warns inside the path dialog instead of asking for confirmation, there is no
  recent-files list and no autosave. `apps/furnicraft` still starts from `sample_session()`; a new
  empty scene is one menu click away.
- `[types]` Parametric cabinets do not exist yet: M6 provides classification and properties, and
  product prototypes arrive later as user scripts over those properties.
- `[types]` A component has no separate "dissolve" operation yet: `node.component.convert_to_group`
  clears its marker, prototype keys and furniture type and then flattens it into the parent. There is
  no way to keep the component and detach its prototype record on its own.
- `[types]` The property catalogue is not designed: user properties are free-form and the inspector has
  no type editor for `Structured` values yet.
- `[types]` Furniture types have no registry-backed picker: the inspector edits the raw `GG.TT.VV` code
  and the scene tree shows the stored code, while names and ancestry live in
  `docs/domain/furntypes.md`.
- `[types]` Queries exist in the model and the scene tree selects a furniture-type subtree, but there
  is no engine command or saved report over a query yet.
- `[gizmo]` The gizmos do not snap: translate and rotate angles are free, and snapping is not
  implemented yet.
- `[gizmo]` The rotate rings are world-aligned only; a local-axis mode is not implemented.
- `[gizmo]` The pivot is edited only in the inspector; there is no viewport pivot-pick tool yet.
- `[gizmo]` Move and Rotate act on the top-most selected nodes; the viewport has no Ctrl+click
  multi-select yet, so a multi-selection comes from the scene tree.
- `[gizmo]` The orientation widget is hidden in a viewport smaller than its own size plus margins;
  there is no menu fallback for the six views yet.
- `[panel]` Region rings are traced for rectangles only; a shaped core has no contour-aware cutout
  union, so adding machining to a shaped panel is still rejected.
- `[panel]` The core outline at a line-arc or arc-arc joint still joins per-segment offsets with a
  straight connector; only the band strips are intersected analytically.
- `[panel]` Band trimming relies on 3D booleans; when truck finds no solid the earlier band is rebuilt
  untrimmed instead of failing the build.
- `[panel]` A legacy file can still carry a corner banding on a point corner; the card does not show it
  and it is removed when the corner next becomes a segment.
- `[panel]` Blind cutouts that overlap any other cutout or cross the core edge are still rejected until
  the 3D edge-pocket stage.
- `[panel]` Included offsets on a shaped path are checked only through the core extents; a hostile band
  thickness can still make the core contour self-intersect instead of returning a typed error.
- `[panel]` `PanelGeometry` reports shaped core extents with a zero core origin; the body and cutout
  tools use nominal coordinates, but cutout validation on a shaped panel only sees the core bounding
  box.
- `[panel]` The panel card does not edit cutouts yet.
- `[panel]` Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks
  for some checks; rendering and diagnostics use the catalogue.
- `[vendor]` `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
