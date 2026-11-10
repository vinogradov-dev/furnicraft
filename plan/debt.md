# Known debt

Gaps accepted for now. Read the entries whose tag matches the area of the task in hand; the whole file
is not part of a session start. An entry leaves this file when the fix lands.

Tags: `[view-state]` session versus document, `[render]` lighting and drawing, `[mesh]` geometry
editing, `[object]` constructive objects, `[panel]` panels and banding, `[scene]` tree and files,
`[gizmo]` manipulation, `[types]` properties and furniture types, `[engine]` commands and history,
`[perf]` frame and picking cost, `[assistant]`, `[ui]` panels, toolbars and icon artwork, `[design]`
module size, layer arrows and duplicated knowledge, `[vendor]`.

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
- `[object]` A constructive object supports one `Extrude` feature; there are no booleans, fillets or
  other feature kinds yet. Collapse is a one-way door and only undo brings the history back. The
  object card still offers `X, Y` text lines for a line-only ring beside the viewport editor.
- `[object]` A contour's self-intersection test samples each segment into chords; a crossing finer than
  0.1 mm inside an arc can slip through. An exact segment-pair test can replace it when the sketch
  editor needs the last tenth of a millimetre.
- `[object]` An arc stores a rounded centre, radius and angle rather than its two endpoints, so a
  joint at an arc is judged with a radius-dependent slack (ADR 0072). Storing explicit endpoints at
  schema 20 would make the arc end exactly the point the user drew, return the tolerance to one
  micrometre for every pair, and drop the pair rule from `to_geometry` and `joint_tolerance_mm`.
- `[object]` Along-contour movement at a corner shared by two arcs is refused with `ArcJointMismatch`
  when no point sits on both circles at millidegree resolution, because neither stored circle may be
  bent. Free movement rebuilds both arcs and is the default, so the grip only needs to avoid that
  motion at such a corner.
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
- `[perf]` A rebuild reuses unchanged meshes, but `ViewportRenderer::upload` still recreates every GPU
  buffer in the frame, even for drawables whose mesh handle did not change. A buffer cache keyed by
  the mesh handle would make a one-panel edit upload one panel.
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
- `[scene]` A scene written by an earlier build is not migrated to the current frame: there are no
  releases and no user files yet (`plan/rename.md`), so an old pre-release document may load with a
  different world shape instead of being converted. Owner's decision, 2026-11-10; revisit when a
  format ships to a user.
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
- `[gizmo]` The orientation widget is hidden in a viewport smaller than 124 pt wide plus margins
  (148 pt), or 148 pt tall plus margins; there is no menu fallback for the six views or the roll
  ring yet.
- `[gizmo]` An extrusion pull previews its new height as a wireframe prism, not a shaded solid. A
  solid preview needs the evaluation cache to accept a previewed height without a document write.
- `[panel]` The core outline at a line-arc or arc-arc joint still joins per-segment offsets with a
  straight connector; only the band strips are intersected analytically.
- `[panel]` Band trimming relies on 3D booleans; when truck finds no solid the earlier band is rebuilt
  untrimmed instead of failing the build.
- `[panel]` A legacy file can still carry a corner banding on a point corner; the card does not show it
  and it is removed when the corner next becomes a segment.
- `[panel]` Blind cutouts that overlap any other cutout or cross the core edge are still rejected until
  the 3D edge-pocket stage.
- `[panel]` The panel card does not edit cutouts yet.
- `[panel]` A contour banding row names its ring segment by number only; the card cannot highlight the
  segment in the viewport, so the user has to count segments to band the one they mean.
- `[panel]` Command validation (`panel.resize`, `panel.edge_banding.set`) still uses stored fallbacks
  for some checks; rendering and diagnostics use the catalogue.
- `[panel]` A bent panel is not implemented. The agreed construction is a flat panel with all its
  machining, then a bend applied to the finished body; the operation pipeline needs a post/deform
  stage and the panel card a bend section. No geometry is stored until that stage exists.
- `[vendor]` `vendor/truck-shapeops` is updated by hand when `truck` moves; see `vendor/AGENTS.md`.
- `[ui]` A creation tool lands at the scene root since X4; a node inside a container needs the
  explicit *Add child* arm of U31. Until then the only way is to create at the root and move the row
  in the tree.
- `[ui]` The ribbon's *Model* tab wants a cone beside the cube, cylinder and sphere, but
  `PrimitiveKind` has no `Cone` and `crates/param` builds no cone solid, so the tab draws the three
  primitives the document really has. A cone needs the model variant, the kernel plan and the drawn
  `primitives/cone` asset in one change; drawing the icon alone would offer a tool that cannot build.
- `[ui]` 47 of the 72 declared icons are zero-byte placeholders: the whole `actions` theme (30,
  including undo, redo, delete, save, warning), `primitives/cone`, `bodies/revolve` and
  `bodies/sweep`, and the ten view glyphs U40 added for the floating cluster (orbit, pan, zoom,
  layout, sliders and the five shading styles). The empty-file fallback keeps every surface usable
  with its text label, but the cluster wraps onto several rows in a narrow viewport until U46 draws
  them. See `assets/icons/AUDIT.md` §5 and ADR 0055.
- `[ui]` The regular polygon's `Sides` field has no refusal state: a value outside `3..=128` is
  clamped and a non-number falls back to six, while the field keeps the typed text. The tool builds
  a valid polygon instead of failing, but a typo is not named. A validated field can land with the
  polygon dialog, if one is ever added.
- `[ui]` A ring exposes corner grips only: the shape tag lives on `Path`, so a panel outline or a
  cutout ring has no circle, rectangle or polygon grips and no arc radius handle even when its
  segments are arcs. Ring-level shape metadata is needed before those handles can exist.
- `[ui]` The number field opens when a grip drag starts, not when a grip is merely hovered or
  clicked, so typing a value without moving the pointer first needs a tiny drag. A click-only path
  would need the field to survive a press that never becomes a drag.
- `[ui]` The pull arrow opens only when the ribbon's *Extrude* button creates the body: the same
  `object.path.extrude` from the command palette or an API, and `object.extrude.create` from a
  contour, leave the body without a pull. The shell reads the height from the command result, so
  another entry point needs the same signal before it can pull too.

- `[design]` `apps/furnicraft/src/main.rs` `viewport` is one 635-line function. U45 moves it onto
  `Document` with the rest of the scene-bound state, so the S7 audit filed no split; it is measured
  again and split there.
- `[design]` `impl FurniCraftApp` is 1620 lines over 40 fields. U45 extracts about twenty fields into
  `Document` and the viewport and file state into `tabs.rs`, so the audit filed only the two moves U45
  does not perform (S21 Home, S22 the chrome) and the impl is re-measured after U45.
- `[design]` `ui/panel_card.rs` (989), `ui/object_card.rs` (989) and `ui/toolbars.rs` (982) sit under
  B4's 1000-line ceiling and their impls under 800 (`PanelCard` 432, `ObjectCard` 469, and
  `show_group_row` 146 is under the 150-line function ceiling), so the audit filed no split although F6
  named them. Re-measure when a section grows.
- `[design]` Twenty modules carry 25 or more public items (B6). Each is one value family or one command
  family whose header states what holds it together (`geom` segments, `model/contour.rs`,
  `model/primitive.rs`, `engine/node_cmds.rs`, `model/panel/machining.rs`, `ui/theme.rs`); the audit
  filed only the two over B4's size ceiling (S9, S14).
- `[design]` The task report selects `plan/debt.md` and `plan/questions.md` entries by the task's
  `files:` paths, their crate directory, a file stem and the crate tag (`[engine]`); an entry that
  names the area in prose only is not selected, and `cargo xtask task --all` prints every entry.
  Keeping the tags and the `files:` hints honest is what makes the selection useful (P4).
