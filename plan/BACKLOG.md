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

- `[x]` **U27** the theme reaches the icon: `icon_widget` rasterises an embedded white master with
  `egui_extras` at `pt × pixels_per_point`, bakes `WidgetVisuals::fg_stroke.color` into the pixels,
  caches a `TextureHandle` per `(uri, px, colour)` and paints it through `Painter::image`; the frame,
  hover/press and disabled alpha come from the same widget visuals a text button uses. Tools draw at
  20 pt and inline buttons at 16 pt; the 18 masters and the format docs say `#fff`, and the toolbar
  tests read the accessible name instead of `Image`'s alt text. The pixel test draws one icon in both
  themes and asserts the theme colour, one alpha silhouette and `round(pt × ppp)` at 1.0, 1.25 and
  2.0. ADR 0055 — `84a4675`
  `files: crates/ui/src/icons.rs, assets/README.md`

- `[x]` **U37** a cutout ring is edited in the viewport: `ContourTarget::Cutout { panel, machining }`
  reads the ring from `panel.machining_by_id`, `ContourEdit::apply` rebuilds the record through one
  `panel.machining.update` while keeping the anchor and the `CutoutDepth`, and the cutout card puts
  *Edit contour* beside the ring summary. `ContourEntry` names the editing, busy, ready and
  single-selection states, so the outline and the cutout never both claim the one live edit. Tests:
  an edit is one revision and one undo entry, the anchor and depth are unchanged, and a ring outside
  the core is refused while the draft stays — `65b1d31`
  `files: crates/ui/src/machining/cutout.rs, crates/ui/src/contour_edit.rs`

- `[x]` **U36** a rectangle panel outline becomes a ring once: `PanelOutline::Rect` stores corners and
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
  features; a second conversion is refused. ADR 0061 — `8ae7de5`
  `files: crates/engine/src/panel_cmds.rs, crates/model/src/panel/contour.rs`

- `[x]` **U35** the path gizmo: the contour gizmo painted every draft as a closed ring and put a handle
  on each segment start, so an open chain lost its last point and no free end could be grabbed.
  `ContourGizmo` now paints a `ContourDraft`: a closed ring and a closed chain paint as
  `Shape::closed_line`, an open chain as `Shape::line`, and the open end gets a `PathCorner::End`
  handle. Handles, the drag and the arc projection are addressed with `PathCorner`, so a drag on the
  last point edits the last segment and the free start and free end project against their one
  segment. The object card offers *Edit contour* for a path as well as a constructive object — `f73a8ab`
  `files: apps/furnicraft/src/contour_gizmo.rs, crates/ui/src/object_card.rs`

- `[x]` **U34** the path edit mode in the editor: `ContourTarget` gains `Path(Id)` and `ContourEdit`
  holds a `ContourDraft` that is either a ring or a chain, so an open line or a circle drawn by the
  tools has an editor. Moving, splitting and resetting a chain go through the model's `PathCorner`
  helpers, `apply` dispatches one `object.path.replace`, and `start`, `is_editable`, `is_dirty`,
  `reset` and `retain_contour_edit` keep their meaning for both drafts. The open chain is painted
  and dragged by the path gizmo of U35; the ring gizmo ignores a chain draft — `3970c0b`
  `files: crates/ui/src/contour_edit.rs, crates/ui/src/lib.rs`

- `[x]` **U33** path edit math in the model: a ring names a corner by the segment that starts on it,
  which wraps, so an open chain's last point had no name. `PathCorner::Start` and `End` name a corner
  explicitly; `Path::move_corner_at`, `split_segment_at`, `remove_segment` and `close` share
  `with_corner`, `join_corner` and `split_document` with the ring code. Ids are never reused,
  `Path::rebuild_as` keeps the counter while `close` flips the flag, a removal joins the neighbours at
  the previous end, and `close` refuses a bridge that would cross the chain (ADR 0060) — `7d6ca68`

- `[x]` **X5** a selected line, contour or circle is invisible: a selection tint moved a chain's
  channels by less than the eye resolves, and a one-pixel `LineList` cannot be widened in wgpu. A
  selected chain now takes one explicit `SELECTION_LINE_COLOR`, and the shell keeps the selected
  chains of the frame it just uploaded — after the drag preview — and repaints them over the viewport
  image: a dark 3.5 pt underlay under a 2.25 pt bright stroke. A pure `selection_outline` projects the
  stored points and cuts a chain behind the camera; the renderer keeps its one-pixel pipeline
  (ADR 0059) — `0e289e0`

- `[x]` **U32** a new flat figure pivots at its centre or at its first point: `Path::creation_pivot`
  and `Contour::creation_pivot` return the local bounds centre for a closed figure and the start of
  an open chain's first stored segment; the midpoint is summed in `i128`, so two extremes near the
  coordinate limits cannot overflow, and a flat figure's pivot lies on `z = 0`.
  `Project::add_object_with_pivot` is the only creation path that takes a pivot, `add_object`
  delegates with `Pivot::ZERO`, the path and extrusion commands use it, a replacement leaves it
  alone, and an older file that stores no pivot loads with zero. ADR 0058 — `bef8527`

- `[x]` **U31** a child is created through an explicit arm: `Editor` holds `creation_parent`,
  `creation_target(project, armed)` falls back to the root when the armed id vanished, the container
  menu and card arm a group or an assembly, and the strip shows *Adding to: Frame ✕*; Escape and the
  container's removal clear the arm, while a primitive resolves its parent at the click — `478ba44`

- `[x]` **X6** the top and bottom views look at an angle to their plane: `MIN_PITCH`/`MAX_PITCH` are
  ±85° and `look_from` clamps to them, so a click on the Z ball left the camera five degrees off
  vertical and the grid, the work plane and the drawing all skewed. The limits are now `±FRAC_PI_2`;
  `right()` is analytic, `(-sin yaw, cos yaw, 0)`, the exact limit of the old `forward × Z`, and
  stays unit at the pole; `up()` is `right × forward` with no fallback; `view_matrix` uses `self.up()`,
  so an authored camera's pinned up is not overwritten. Tests cover both poles, the centre ray, the
  target projection, the work plane, the widget flip and an orbit off the pole. ADR 0057 replaces the
  five-degree clamp of ADR 0019 (`Accepted; pole clamp superseded by 0057`) — `7b0a02e`

- `[x]` **U30** the armed tool is highlighted: `icons::tool_button` takes `on: bool` and draws
  `Button::selected`, which egui paints with `visuals.selection.bg_fill`, and the text fallback
  takes the same flag. `ToolActivity` names the two arming tools and the pure `tool_activity`/
  `is_armed` selectors press exactly one button; the strip takes `active` and the editor derives it
  from the running draft or the waiting primitive. A one-shot creation never arms. The duplicate
  "Draw" row is gone, so the strip is the only creator; toggled-on stays a theme state, not a second
  SVG (ADR 0055) — `78b4d76`

- `[x]` **X4** every creation tool lands at the scene root: `creation_root(project)` is the only
  default parent, `ToolAction` carries no parent and `drawing_parent` is gone, so a selected nested
  assembly receives none of the six creations; `extrude_path` stays a sibling of its path. ADR 0056
  supersedes the parent rule of ADR 0052 — `c135511`

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
