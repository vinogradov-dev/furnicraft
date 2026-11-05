# Backlog

One entry per task, stable id in front; a long entry may wrap onto indented lines. `[ ]` open, `[~]`
in progress (exactly one, in `## Now`, matching `plan/SESSION.md`), `[x]` done and in `## Done` with
the commit that closed it. Older entries are kept as a short tail so the current position reads at a
glance.

The `files:` hint names where the work starts and is required on every unfinished task: each path
must exist, so name a directory for a file the task creates. The hint is a hint, not a boundary: read
those files and their callers, not the workspace.

Order is the order of work. Renumbering is not allowed; a new task takes the next free id in its
phase. The first unfinished task in `## Now` is the one `STATE: task=` must name; `## Bugs` and
`## Next` are not part of that order.
Phases come from the shape-base plan: A contour, B contour operations, C sketch, D panel
variants, E 2D primitives, F revolution and sweep, G 2D booleans, H hardware and accessories; P is
the plan and `xtask` itself, L is scene lighting, S is a structural refactor that changes module
boundaries only, V is primitive parameters, X is a defect in behaviour that already shipped, and U
is the UI: toolbars, drawing tools, view controls and icon resources.

A defect is filed under `## Bugs` with an `X` id next to the feature that owns it. Its entry states
the observation, the expectation and the suspected cause, and never a guess at a fix that has not
been reproduced. Taking it moves the line into `## Now` at its place in the work order, so `STATE` can
name it; fixing it moves it to `## Done` with the commit, like any task.

## Now

- `[~]` **X4** every creation tool lands at the scene root, never in the selected container. Today
  `crates/ui/src/lib.rs:815-824` `drawing_parent` returns the selected container, or the parent of the
  selected leaf, and both `commit_drawing` (`:361`) and `show_toolbars` (`:470`) feed it into
  `ToolAction::Create*`. A path, a panel, an assembly, a primitive, a camera and a light therefore land
  in whatever group is selected, which the owner does not want. Expected: the root is the only default
  parent; a child is created by an explicit entry (U31). Drop `parent` from `ToolAction`
  (`crates/ui/src/toolbars.rs:26-64`), from `show`/`show_panel` and from `drawing_parent`; one function
  `creation_root(project) -> Id` stays. `extrude_path` (`lib.rs:585-596`) keeps making a sibling of its
  path: it is a derived operation, not a tool. Tests: a selected nested assembly receives none of the
  six creations; `crates/ui/src/lib.rs:1090-1130` becomes the regression that asserts the root. ADR
  0056 replaces the parent rule of ADR 0052, whose status becomes
  `Accepted; parent rule superseded by 0056`.
  `files: crates/ui/src/lib.rs, crates/ui/src/toolbars.rs`

- `[ ]` **U30** the armed tool is highlighted: choosing Line, Arc, Polyline, Rectangle, Circle or a
  primitive leaves its button looking idle, so the strip shows the armed tool only in its text
  (`main.rs:925-959`). `icons::tool_button` (`crates/ui/src/icons.rs:225-233`) gains `on: bool` and
  draws `Button::selected(on)`, which egui paints with `visuals.selection.bg_fill`; the text fallback
  takes the same flag. `toolbars::show`/`show_panel` (`crates/ui/src/toolbars.rs:278-374`) take
  `active: Option<ToolActivity>` (`Draw(DrawTool) | Primitive(PrimitiveKind)`), and the editor passes
  `drawing().map(DrawDraft::tool)` or `pending_primitive().map(kind)`. Delete the duplicate "Draw"
  button row at `main.rs:975-981`: U20 made the toolbar the only creator, so it is a second source of
  truth. Toggled-on is a theme state, not a second SVG (`assets/icons/STYLE_GUIDE.md`, ADR 0055).
  Tests: the selector is a pure function with a table; one egui run marks exactly one button, and an
  idle editor marks none.
  `files: crates/ui/src/icons.rs, crates/ui/src/toolbars.rs`

- `[ ]` **X6** the top and bottom views look at an angle to their plane: `MIN_PITCH`/`MAX_PITCH` are
  ±85° (`crates/scene/src/lib.rs:26-31`) and `look_from` clamps to them (`:438-449`), so a click on the
  Z ball leaves the camera five degrees off vertical and the grid, the work plane and the drawing all
  skew. Expected: the camera axis is perpendicular to the plane of the view (owner's answer, was an
  open question). Set the limits to `±FRAC_PI_2`; make `right()` (`:240-245`) analytic,
  `(-sin yaw, cos yaw, 0)`, which is the exact limit of today's `forward × Z` and stays unit at the
  pole; `up()` (`:248-253`) is then `right × forward` with no fallback; `view_matrix` (`:256-279`) uses
  `self.up()` instead of recomputing `right.cross(forward)`, which also stops an authored camera's
  pinned up from being overwritten. Tests: `look_from(Z)` gives `pitch == π/2` and `forward == -Z` with
  a unit finite `up`; `ray_ndc(0, 0)` is `-Z`; the target projects to the viewport centre;
  `WorkPlane::from_camera` (`apps/furnicraft/src/gizmo_math.rs:419-444`) selects XY from both poles;
  `gizmo::snap_direction` still flips at the pole (`apps/furnicraft/src/gizmo.rs:115-121`); an orbit
  off the pole moves. Manual check: grid, gizmo and picking in both pole views. ADR 0057 replaces the
  five-degree clamp of ADR 0019 (`Accepted; pole clamp superseded by 0057`).
  `files: crates/scene/src/lib.rs, apps/furnicraft/src/gizmo_math.rs`

- `[ ]` **U31** a child is created by an explicit entry, not by the selection: pick a group or an
  assembly in the tree or in the viewport, choose *Add child*, and the next created node lands inside
  it. `Editor` gains `creation_parent: Option<Id>` — view state, never the document
  (`plan/debt.md` `[view-state]`). Entry points: `TreeAction::AddChild { parent }` in the container
  menu (`crates/ui/src/scene_tree.rs:294`), a button in `crates/ui/src/container_card.rs`, and the
  strip chip "Adding to: Frame ✕". `creation_root` from X4 becomes
  `creation_target(project, armed)`, which falls back to the root when the armed id vanished. The arm
  is sticky until ✕, Escape or the container being deleted, so filling an assembly is one arm and
  several tools; clearing it on a selection change was rejected because the selection moves to the new
  node after every create. `after_node_removed` (`crates/ui/src/lib.rs:776`) already drops a dead
  selection and must drop this too. The root cannot be armed. Tests: arm an assembly, create a path, a
  panel and a primitive — all three are children; ✕ restores the root; deleting the container clears
  the arm. ADR 0056 covers this rule together with X4.
  `files: crates/ui/src/lib.rs, crates/ui/src/scene_tree.rs`

- `[ ]` **U32** a new flat figure pivots at its centre or at its first point: every path and body is
  created with `Pivot::ZERO` (`crates/model/src/object.rs:288-292`, `add_object`,
  `crates/model/src/project/object_cmds.rs:31-50`), so the gizmos grab a drawing at the world origin
  instead of at the shape. Expected: a closed contour pivots at the centre of its local bounding box;
  an open chain pivots at the start of its first stored segment. Owner's answer: bounding-box centre,
  the same centre `node.pivot.center` writes, and the extruded body follows the same rule. Add
  `Path::creation_pivot()` and `Contour::creation_pivot()` (`crates/model/src/contour.rs`; integer
  micrometres, `bounds_micrometres` plus an `i128` midpoint, open → first segment `start()`),
  `Project::add_object_with_pivot` (the old `add_object` delegates with `ZERO`), and use it in
  `object.path.create` and `object.extrude.create`
  (`crates/engine/src/object_cmds.rs:168-188, 279-288`) — one command, one revision, one undo entry,
  no new parameter to lie about. A primitive keeps `ZERO`: its bounds are already centred.
  `object.path.replace` and `object.contour.replace` must not move the pivot (ADR 0026), and an older
  file keeps the zero it stored. An arc's first point is the stored `segment.start()`, never the raw
  click. Tests: rectangle and circle centre; open polyline and open arc start; a replacement leaves the
  pivot alone; a loaded file without the field is zero. ADR 0058.
  `files: crates/model/src/contour.rs, crates/engine/src/object_cmds.rs`

- `[ ]` **X5** a selected line, contour or circle is invisible: the highlight exists
  (`SceneFrame::with_selection_highlight`, `crates/scene/src/lib.rs:741-757`) but `SELECTION_TINT`
  mixed at 35 % into `PATH_COLOR = [0.95, 0.72, 0.20]`
  (`apps/furnicraft/src/scene_build.rs:35`) moves green by 0.06, which the eye cannot see, and the
  renderer draws a chain as a one-pixel `LineList` (`crates/render/src/lib.rs:261-295`) that cannot be
  widened in wgpu. Expected: a selected drawing reads at a glance and stays readable on a brown panel
  and on a light one. Two changes: polylines get one explicit `SELECTION_LINE_COLOR` instead of a tint,
  and the shell paints the selected chains as a screen-space overlay over the viewport image (2–2.5 pt
  with a dark 3.5 pt underlay), the way camera glyphs and the mesh selection are already painted
  (`apps/furnicraft/src/main.rs:726-734`). `upload_scene` keeps the selected chains of the frame it
  uploads, after the drag preview, so the outline follows a move without lagging; the renderer keeps its
  one-pixel pipeline. Tests: the scene colour test; a pure `selection_outline` that projects the stored
  points without a GPU. Manual check on both panel colours. ADR 0059 records the UI-overlay boundary.
  `files: crates/scene/src/lib.rs, apps/furnicraft/src/main.rs`

- `[ ]` **U33** path edit math in the model: the edit functions only know a ring. `move_corner`,
  `trim_segment` and `split_segment` (`crates/model/src/contour_edit.rs:73-171`) address a corner by the
  segment that starts at it, and `position` (`:174-177`) walks a modulo ring, so an open chain has no
  handle on its last point and would weld the last segment to the first. Add
  `PathCorner { Start(ContourSegmentId), End(ContourSegmentId) }` and the same operations on `Path`:
  `move_corner_at`, `split_segment_at`, `remove_segment` (open chains join the neighbours) and `close`,
  sharing `with_corner`, `join_corner` and `split_document` (`:185-273`) with the ring code. The
  invariants do not move: ids are never reused, an arc keeps its centre and radius, a joint is exact to
  one micrometre, and every result is rebuilt through `Path::rebuild`, which keeps the counter and the
  closed flag. Tests: an open polyline moves its first and its last corner, an arc moves along its
  circle, removing a segment joins its neighbours, closing is refused when the chain would
  self-intersect, and a corner off the support is refused. ADR 0060.
  `files: crates/model/src/contour_edit.rs, crates/model/src/contour.rs`

- `[ ]` **U34** the path edit mode in the editor: `ContourTarget` only accepts a panel ring and a
  constructive contour (`crates/ui/src/contour_edit.rs:26-49`), so an open line or a circle drawn by the
  drawing tools has no editor. Add `ContourTarget::Path(Id)` and let `ContourEdit` hold either a
  `Contour` or a `Path` draft; `apply` (`:121-132`) dispatches `object.path.replace`
  (`crates/engine/src/object_cmds.rs:290-329`, already exists) and keeps one revision and one undo
  entry. `start`, `is_editable`, `is_dirty`, `reset` and `retain_contour_edit`
  (`crates/ui/src/lib.rs:242-300`) keep their meaning for both drafts. Tests: editing an open path is
  one revision and one undo; cancel restores the stored chain; the closed flag survives an edit; a
  stale target ends the mode.
  `files: crates/ui/src/contour_edit.rs, crates/ui/src/lib.rs`

- `[ ]` **U35** the path gizmo: `contour_gizmo` paints a closed ring (`Shape::closed_line`,
  `apps/furnicraft/src/contour_gizmo.rs:178-200`) and puts a handle on every segment start
  (`:203-224`), which loses the last point of an open chain. Paint an open chain as `Shape::line`, add
  the end handle, and address handles with the `PathCorner` of U33 so a drag on the last point edits the
  last segment. The object card's `contour_edit_section` (`crates/ui/src/object_card.rs:507-535`)
  currently shows only for a constructive object — show it for a path too. Manual check: line,
  polyline, arc, rectangle and circle all move a corner and split a side; Enter applies, Escape cancels.
  `files: apps/furnicraft/src/contour_gizmo.rs, crates/ui/src/object_card.rs`

- `[ ]` **U36** a rectangle panel outline becomes a ring once: `PanelOutline::Rect` stores corners and
  bows (`crates/model/src/panel/mod.rs:49-107`) and has no ring to edit, so *Edit contour* is offered
  only for a panel that is already a ring (`:194-207`). Add `panel.outline.to_contour`
  (`crates/engine/src/panel_cmds.rs`) that bakes the current outline through `Panel::nominal_contour`
  (`crates/model/src/panel/contour.rs:165`) in one revision and one undo entry, then opens the ordinary
  edit mode. The hard part is banding: a ring panel cannot keep `EdgeBanding` or `CornerBanding` (the
  loader refuses `ContourWithBanding`, `crates/model/src/panel/mod.rs:899-903`), so the command
  translates each edge band and each corner band into the ring segments `contour_with_sources_for` says
  it produced, and refuses with a readable reason when a corner band has no unique ring source. The card
  warns that the corner and bow parameters are gone; undo brings them back. Tests: the ring keeps the
  outline bounds; the converted banding names the same segments; undo restores the rectangle with its
  features; a second conversion is refused. ADR 0061.
  `files: crates/engine/src/panel_cmds.rs, crates/model/src/panel/contour.rs`

- `[ ]` **U37** a cutout ring is edited in the viewport: a `CutoutShape::Contour`
  (`crates/model/src/panel/machining.rs:390-414`) shows only a one-line summary
  (`crates/ui/src/machining/cutout.rs:64-73`) and nothing can reshape it. Add
  `ContourTarget::Cutout { panel, machining }`: `contour` reads the ring from `panel.machining_by_id`
  (`crates/model/src/panel/mod.rs:358`) and `apply` rebuilds the record with `panel.machining.update`
  (`crates/engine/src/panel_cmds.rs:348-385`), keeping the anchor and the `CutoutDepth`. The card gets an
  *Edit contour* button beside the ring summary. Tests: an edit is one revision and one undo entry; the
  anchor and the depth are unchanged; a ring that no longer fits the core is reported on the operation,
  not silently stored.
  `files: crates/ui/src/machining/cutout.rs, crates/ui/src/contour_edit.rs`

- `[ ]` **U27** the theme reaches the icon: `crates/ui/src/icons.rs` rasterises an embedded master
  instead of handing it to `Image::tint`, whose `vertex_colour × texture` product makes a `#000`
  master black in every theme. Rasterise with `egui_extras::image::load_svg_bytes_with_size` at
  `pt × pixels_per_point`, cache a `TextureHandle` per `(uri, px)`, paint through `Painter::image` in
  `visuals.widgets.inactive.fg_stroke.color` (hover/press from the widget state, disabled an alpha
  multiply). Masters switch to `#fff`; toolbar sizes split into 20 pt tools and 16 pt inline. A pixel
  test draws one icon in both themes and asserts the theme colour in the uploaded pixels, an
  identical alpha silhouette and a raster of `round(pt × ppp)` — it fails on today's code. ADR 0055,
  `assets/README.md` format, `files: crates/ui/src/icons.rs, assets/README.md`

- `[ ]` **U28** mass and one projection: allow `fill-opacity` (still no `defs`, `mask`, `clipPath`,
  gradient or CSS) so a passive face fills at 12–18 % and the result or changed geometry at 45–60 %
  with the heavy outline, then redraw the 18 drawn icons at a fixed footprint (ink `3..21`, coverage
  20–45 %) on the one axonometry the guide publishes. One change updates `STYLE_GUIDE.md` §5, §7.3 and
  §9, the draw-icon skill and a coverage test read off the rasterised alpha. ADR 0055;
  `files: assets/icons/STYLE_GUIDE.md, crates/ui/src/icons.rs`

- `[ ]` **X2** a valid contour with more than 1024 sampled points cannot become an extrusion.
  Observed: the command now refuses it with "one extrusion cap holds at most 1024", where before
  A6 it was stored and drew nothing. Expected: any simple ring the user can draw becomes a solid.
  Cause: one extrusion cap is one document face, capped by `MAX_FACE_VERTICES`; the ring samples at
  50 µm, so a smooth ring of a few turns exceeds it while the ring itself is legal (at most 256
  segments). `object.collapse` fails on such an object and `object_mesh` cannot draw it.
  `files: crates/param/src/lib.rs, crates/model/src/object.rs`

- `[ ]` **U29** an arc inside the polyline: the Polyline tool appends either a straight segment or a
  two-click arc, and a chain that mixes both still closes and extrudes. The arc leaves the last
  vertex: the first click is the point it passes through, the second is its end, and the next
  segment starts where the stored arc actually ended. `DrawDraft` therefore stops holding a point
  list and holds the segments it has built, so `to_path` becomes a rebuild and every vertex after an
  arc is the stored `end()`, not the raw pointer point. That last rule is the trap: `arc_through`
  rounds centre, radius and angle to whole document units, `Path::assemble` tolerates a 1 µm joint
  while `Contour::assemble` demands exact equality, so a chain built from unrounded clicks can draw
  in the viewport and then fail `to_contour` — the only way to an extrusion. The draft also needs a
  third state, an arc waiting for its end, which `push`, `undo` (cancel the pending arc before the
  last segment), `is_complete`, `preview_path` and `size_label` all have to know; closing still adds
  one straight segment from the last stored end to the first stored start, and the pointer keeps
  snapping to the first vertex to close. A dragged bulge and a tangent continuation were rejected
  for now: the gizmo owns clicks, not drags. No document change — no schema, no command, no undo
  entry — and ADR 0048 gains one sentence saying a drawn vertex follows the stored arc end. Tests: a
  path whose arc end misses the next line by one micrometre converts through `to_contour` and by two
  is refused, a draft mixes both kinds and closes, an arc endpoint becomes the next vertex, undo
  cancels a pending arc, and the commit is still one `object.path.create`.
  `files: crates/ui/src/contour_edit.rs, apps/furnicraft/src/draw_gizmo.rs, crates/model/src/contour.rs`

## Bugs

## Next

## Done

- `[x]` **X3** the projection toggle draws the mode it is in, not the one it switches to: the icon
  and the hint come from one pure `projection_button(Projection)`, so a perspective view shows the
  perspective glyph and the hint names the switch; a table test covers both modes and the
  authored-camera disable stays where it was — `9bebec0`

- `[x]` **V1** a primitive keeps the parameters that built it: `Geometry::Primitive(Primitive)`
  stores the shape and its parameters, `crates/model/src/primitive.rs` validates them on construction
  and on load, `crates/mesh/src/primitive.rs` derives the mesh, `object.primitive.create` stores the
  recipe and `object.primitive.replace` edits it in one revision and one undo entry;
  `object.collapse` bakes either a constructive or a primitive payload. Schema 18, the
  `scene-v18.fcs` golden and ADR 0054; a cube keeps six faces and only the curved shapes carry a
  face count — `56b876c`

- `[x]` **U26** the toolbar icon rollout: cube, cylinder and sphere (`97ee5ce`), then panel,
  assembly, camera, light and extrude; the viewport strip draws frame-all and the projection toggle
  from `view/*`, and one test rasterises every drawn asset as a 24 grid SVG — `3fd6ed1`

- `[x]` **P3** `STATE: task=` must name the first unfinished task in `## Now`: `first_unfinished`
  takes the first not-`[x]` line there, a later task is a violation that names the skipped one,
  `## Bugs`/`## Next` stay outside the order, and a stray `[x]` in `## Now` keeps its own error —
  `0190f91`

- `[x]` **P2** `updated=` is a `YYYY-MM-DD` calendar date and never earlier than the `base` commit's
  date: `calendar_date` checks the shape and the month, `Git::commit_date` reads `%cs`, and the
  comparison runs only when the base resolves — `38057a2`

- `[x]` **P1** `cargo xtask status` prints `head <short hash>` and, while `state=doing`, the note's
  `task`/`now`/`blocked`/`run` block, so an interrupted session resumes in one command: `Summary`
  carries the parsed note and the head commit, and only a backtick pair wrapping a whole note value
  is unwrapped — `b03e88b`

- `[x]` **S4** revision-keyed evaluation cache: `(Revision, Id)` to evaluated body and mesh in
  `furnicraft-mesh`; `scene_build` and `object.collapse` share one evaluator and a rebuild touches
  only changed nodes. The key is the node id plus the derivation inputs, because the revision is
  document-wide and would miss on every edit; `finish` prunes unvisited nodes — `42fc9a6`
- `[x]` **U23** extrude a selected closed path: a height field and one Extrude button in the Bodies
  panel, `Path::to_contour` under `object.extrude.create`, the body a sibling at the path's
  placement, and an open path refused with a readable reason — `098b4a3`
- `[x]` **U22** cylinder and sphere: `Mesh::cylinder` and `Mesh::sphere`, their `PrimitiveKind`
  variants and three Primitive-panel buttons — `49a8246`
- `[x]` **U21** a primitive from its panel: an edge-length field, a Cube tool that arms a placement,
  and one `object.primitive.create` where the work plane is clicked — `b1912b4`
- `[x]` **U20** the scene tree only shows and restructures: every create button and `TreeAction`
  variant is gone and the toolbar strip is the only creator — `8a099f1`
- `[x]` **U19** fill the toolbar panels: five contour tools, four scene creators, the cube and the
  extrude, each a hinted icon button with a text fallback and shared shortcuts — `14b2c76`
- `[x]` **U18** themed toolbars: `ToolPanel` and `Toolbars` group the tools by theme as collapsible
  sections, with a hinted tool button and a text fallback — `d38046b`
- `[x]` **U17** commit a drawn path: one `object.path.create` on the work plane, the node selected
  and the tool reset, in one undo entry (ADR 0052) — `a8e75b6`
- `[x]` **U16** the viewport preview: `DrawGizmo` draws the draft, the snap marker and a live size
  while it owns the pointer, and the strip starts each tool — `b203763`
- `[x]` **U15** the drawing tools: line and polyline, an arc, a rectangle and a circle each build a
  `Path` through `Path::polyline`, `arc_through`, `rectangle` and `circle_through` — `5c487dd`
- `[x]` **U14** a drawing draft in the UI: `DrawDraft` collects plane points, undoes the last
  action, closes into a ring and converts to a `Path`, with the document untouched — `307d745`
- `[x]` **U13** snapping on the work plane: a draft end, an axis lock and the 10 mm grid as pure
  functions, with deterministic ties and table tests — `6d64dc7`
- `[x]` **U12** the work plane: `WorkPlane` maps the pointer onto XY, XZ or YZ from the six standard
  ortho views, world XY otherwise, with a strip label — `6c61962`
- `[x]` **U11** orthographic ↔ perspective as view state: `Camera::toggle_projection` keeps the
  visible height, the strip has a button and `O`, and an authored camera is untouched — `c195d45`
- `[x]` **U10** shape constructors: `Path::rectangle` from two corners, `Path::circle` as four
  quarter arcs and `Path::arc_through`, with degenerate input refused — `13c90b3`
- `[x]` **U9** `object.path.create` and `object.path.replace`, one undo entry each, with ADR 0048 and
  the path payload in the geometry document — `546fb28`
- `[x]` **U8** pick a path: `SceneFrame::pick` measures the ray-to-segment closest approach and
  returns a document `Id` within six viewport points, through one shell helper — `7c544a5`
- `[x]` **U7** draw a stored path: a `Polyline` line primitive in the scene frame, the renderer's
  shared line pipeline, and the shell's sampling of a `Path` into a chain with its bounds — `9ce56be`
- `[x]` **U6** a path object: `Geometry::Path(Path)` at schema 17, with `Project::object_path` and
  `Project::set_object_path`, its chain extents in the node bounds, and older files still
  loading — `847cbbb`
- `[x]` **U5** `Path::to_contour`: a path becomes the ring a panel or an extrusion needs only when it
  is closed and simple, otherwise a readable error — `1c74ea5`
- `[x]` **U4** a flat contour value: `Path` is an ordered chain of line and arc segments with stable
  ids, open or closed, at least one segment, joints meeting within a micrometre and one chain per
  path; tests for a single segment, an arc, an open polyline and a broken joint — `25e330f`
- `[x]` **U25** the contour icon set: `line`, `polyline` and `arc` as flat constructions with
  control points, `rectangle` from two opposite corners, `circle` from its centre, `close` as an
  open chain joined by a heavy segment and `cancel` as the service X; every file reads at 16 px on
  the 24 grid — `d122b85`
- `[x]` **U3** UI scale: a 75–200 % slider in the viewport strip bound to `ctx.set_zoom_factor`, so
  icons are rasterised at the new pixel size instead of stretched, and egui's keyboard zoom stays
  visible in the slider — `152a514`
- `[x]` **U2** icon resources and icon buttons: `egui_extras` with the `svg` feature embeds
  `assets/icons/**` as one module per theme; the shell installs the SVG loader, and `icon_button`
  draws at a fixed 16 pt tinted with the theme text colour, falling back to its text label while an
  asset is empty — `2b453a0`
- `[x]` **U24** the icon contract: one ink on a 24 grid, three families (geometry, view, service), the
  service grammar and its `actions` theme, ADR 0047, and `AGENTS.md` §4.12 plus the `draw-icon` skill
  so the guide is read only while an icon is drawn or edited — `e473342`
- `[x]` **U1** the icon resource tree: `assets/icons/` with one directory per toolbar theme and one
  zero-byte SVG per icon (contour, primitives, bodies, scene, view — 21 placeholders), plus
  `assets/README.md` with the naming, the 24 × 24 grid, the geometry-only format and the empty-file
  fallback — `19fbb49`
- `[x]` **D7** the panel card lists and edits machining from the descriptor table: common operation
  chrome (fold, kind and anchor, reorder, remove) with a section module per kind, an add menu built
  from `MACHINING_DESCRIPTORS` and grouped by category, diagnostics naming a `MachiningId` shown with
  their operation while the Diagnostics section keeps the panel-wide ones, and the open rows kept as
  card view state — `b067bc4`
- `[x]` **D6** the machining pipeline: `CompileMachining` and `OpEffect` (cut region, subtract, add,
  attachment), a `PanelPlan` that folds operations, and a static descriptor table with kind,
  category, stage, capabilities and default; the panel builder contains no kind match, and a
  post/deform stage stays reserved for the deferred bend — `0f1a58a`
- `[x]` **D5** a typed machining list: ordered `Machining` records with stable `MachiningId`s, a
  tagged `MachiningParams` and shared face/edge anchors; `panel.machining.add/update/remove/move`
  replace `cutout.create`, schema 16 writes the record form while legacy cutout entries still load —
  `d87a726`
- `[x]` **D4** one panel outline source: a rectangle keeps its corner and bow parameters as a contour
  generator, a contour panel stores the ring, both produce a `geom::Region`; `PanelGeometry` derives
  core and overall from the region, and no machining code branches on the outline kind —
  `67ccca2`
- `[x]` **G2** exact 2D region booleans on `Contour` rings: union, difference and intersection, used
  to union through cutouts and subtract them from the core instead of the rectangle-only tracer;
  `geom::region` traces the boundary of the set and keeps rings exact — `a0a8e94`
- `[x]` **G1** `geom::Region` holds exact `Contour` rings (outer and holes): rectangle constructor,
  bounds, area, containment and sampling, with the rectangle boolean helpers still passing — `c3c00a4`
- `[x]` **S3** one contour conversion: `crates/param/src/profile.rs` turns a document ring into an
  analytic contour, a sampled `geom::Region` and a `kernel::Profile`; the rectangular core, shaped
  core, cutout tool and constructive extrusion share one sampler and one chord tolerance, and
  `Contour::new_with_tolerance` accepts the one-micrometre joint a stored ring can close to — `82bd63b`
- `[x]` **A7** the contour panel and the contour edit mode are written down in `docs/domain/` and
  ADR 0042: the outline variant, the anchored ring, ring banding, the cutout ring rules and the
  working-copy edit mode — `020a6cb`
- `[x]` **S2** one domain-free solid plan: `SolidPlan` is a base extrusion plus boolean steps, a
  `PlanBuilder` assembles it, and one evaluator drives both the kernel and the analytic
  single-extrusion backend; panels, bands, cutout tools and constructive objects compile to it,
  `object.collapse` keeps the exact mesh, and ADR 0041 records the boundary — `bd33056`
- `[x]` **S1** decompose `crates/model/src/lib.rs` by responsibility: one module per value family,
  panel rules split into geometry, banding and machining, `Project` split into wire, error, derived
  queries and command families, tree traversal in one place; public API, serde and behaviour
  unchanged — `43583b4`
- `[x]` **D3** a cutout by an arbitrary contour: `CutoutShape::Contour`, kept inside the core, through
  and blind, the tool built by extruding the sampled ring; the schema moves to 14 — `2167e2b`
- `[x]` **D2** the contour panel card: the ring summary, the material-driven thickness, the material
  and one banding row per ring segment through `panel.ring_banding.set`/`clear`, no corners, bows or
  cutouts, an `Edit contour` entry; a missing ring-band material is a diagnostic — `f88a995`
- `[x]` **A6** contour edit commands: `panel.outline.set` and `object.contour.replace` share their
  `enabled` check with `run`, validate the ring before the document changes, and record one undo
  entry per apply; a contour too detailed for one extrusion cap is refused instead of stored — `6f3e82b`
- `[x]` **X1** a concave contour cap drew a flap triangle across the notch: `object_mesh` fanned
  every face, so the cap of an extruded object left its own contour; faces are ear-clipped now —
  `a2859fc`
- `[x]` **A3** the contour edit mode: enter on a panel outline or an object contour, edit a working copy
  in the viewport with handles and a preview, leave by applying one command or by cancelling — `1428441`
- `[x]` **A5** contour edit math: move a corner, trim, split, and rely on the segment ring — `dda986c`
- `[x]` **D1** a panel whose outline is a contour ring, banding per ring segment — `d9827ed`
- `[x]` **A4** stable ids for contour segments — `ea3153a`
- `[x]` **A2** contour as a ring of segments — `0833b4c`
- `[x]` **A1** the contour segment type — `e7551ca`
