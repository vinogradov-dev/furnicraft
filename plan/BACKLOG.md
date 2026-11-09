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

- `[~]` **U56** the projection toggle joins the corner widget as a flat icon: the corner orientation
  widget has no projection control, and its Home button is the only control under the cube, drawn as
  a filled plate. Add a 20 pt projection icon in a 22 pt cell directly under the cube, centred, with
  no filled cell at rest — only the glyph — and a faint rounded wash on hover or press; a placeholder
  asset falls back to a frameless text button. It is absent with the widget while an authored camera
  is active. `GizmoAction::ToggleProjection` joins `LookFrom` and `Home`; the shell factors the two
  existing projection arms into one `toggle_view_projection(leaf)` helper, so the ribbon, the
  Display popover and the new button cannot disagree. The readout moves one row down and
  `widget_rect` grows by the button row; the stale "Home is under the cube" comments are fixed.
  Tests: the projection rect is inside the widget and below the cube; a click returns
  `ToggleProjection`; the icon and hint follow the projection; a flat button paints no filled rect
  at rest and one on hover; an undrawn placeholder draws its label.
  `files: apps/furnicraft/src/gizmo.rs, crates/ui/src/icons.rs, apps/furnicraft/src/main.rs`

- `[ ]` **U57** the camera rolls about its own view axis: `Camera` has no roll, so `right()` is
  horizontal, `up()` follows yaw and pitch only, and the readout's roll is always zero. Add a
  `roll: f64` (radians, view state like yaw and pitch, never document data): rotate the analytic base
  `right`/`up` about `forward` by the roll, keeping `up = right × forward`; a positive roll turns the
  image counter-clockwise. `roll()`, `roll_to(f64)` and `roll_by(f64)` normalise into `0..TAU`,
  ignore non-finite input and refuse a pinned `FixedBasis`, like `orbit`. `look_from` and
  `look_from_oriented` reset the roll; `orbit`, `pan_pixels`, `zoom`, `toggle_projection` and
  `with_projection` keep it. `Leaf::roll_to`/`roll_by` clear the preset; presets, Home, Frame all and
  axis balls reset through `look_from_oriented`, orbit keeps. The readout takes
  `signed_degrees(camera.roll())`; ADR 0078 records why the field is on the camera rather than a
  post-multiplied matrix, and ADR 0057 notes that a pole's canonical up is the zero-roll case.
  Tests: +90° sends `right` into `up` and the image of a world point from right to up; the view
  matrix stays orthonormal; picking and panning follow the rolled basis; a pole rolls without moving
  forward, eye or target; snaps reset while orbit keeps; a pinned camera ignores the roll; the
  readout shows the field.
  `files: crates/scene/src/lib.rs, apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/gizmo.rs`

- `[ ]` **U58** the corner widget gains the roll ring and its step setting: the owner wants a
  functional ring that rotates the camera around the view axis. A screen-space ring on a 52 pt
  radius in a 124 pt square around the axis balls, always visible with the widget, with four fixed
  nodes at the top, left, bottom and right carrying 0°, 90°, 180° and 270° and an accent handle at
  the camera's roll. A node click sets the absolute roll (0° resets); a drag anywhere on the annulus
  turns the camera with the pointer, quantised to the step from the workspace settings. The drag
  accumulates the unwrapped visual angle and applies the difference from what was already applied,
  so a slow drag never loses a step and the ±180° seam does not jump; the axis balls keep their hit
  squares and win an overlapping hit. A new `Gizmo` struct owns the drag state; `GizmoAction` gains
  `RollTo(Degrees)` and `RollBy(Degrees)`. The shell passes the step and keeps the ring's drag out of
  the armed navigation and middle-button orbit. `ViewportSettings.roll_step_degrees: u32` defaults to
  1, clamps to `1..=15` and is drawn in the Viewport section as a `DragValue`; the settings JSON
  needs no version bump because the container already has `#[serde(default)]`. Tests: node positions
  and degrees; a 37° drag at step 1 gives 37° and at step 5 gives 35°; a sub-step drag does nothing;
  the remainder carries; the seam does not jump; a node click wins over a drag; the ring hit excludes
  the balls; the handle follows the roll; orbit keeps the roll and a preset resets it; a ring drag
  does not orbit the viewport; the setting clamps and applies live.
  `files: apps/furnicraft/src/gizmo.rs, crates/ui/src/settings.rs, apps/furnicraft/src/main.rs`

- `[ ]` **U45** several scenes open at once: the tab row in the header holds one tab per open file,
  each with its own session, undo history, selection, camera and dirty marker, and activating a tab
  swaps the whole workspace without touching any `.fcs`. Closing an unsaved tab asks first, closing
  the last one leaves an empty *Untitled* scene, and *Save*, *Undo* and *Redo* in the header act on
  the active tab only. This is the one task in the set that changes an architecture boundary rather
  than a look: `Session`, `Editor`, `EvaluationCache`, `FileMenu` and the gizmo state must move from
  fields of the application into one `Document` per tab with its own renderer target, so it gets a
  plan of its own and lands last. Tests: a tab keeps its own undo stack and dirty flag; switching
  tabs restores the selection and the camera; a close with unsaved changes asks and a discard drops only that tab.
  Plan (approved by the owner 2026-11-09; not started): `apps/furnicraft/src/tabs.rs` is the pure
  policy — `TabId`, `Tabs` with the order, the active tab, `activate`, and `close` returning
  `CloseOutcome { Closed, NeedsConfirmation, LastTab }` — testable without a window or a GPU like
  `view_layout`. `apps/furnicraft/src/document.rs` holds the scene-bound payload: `Session`, `Editor`,
  `TruckKernel`, `EvaluationCache`, `SceneFrame`, `scene_revision`, `Layout` (its cameras),
  `home_camera`, `renderer`, `texture_id`, `FileMenu`, `PreservedSections`, the gizmos, `highlighted`,
  `selected_chains`, `uploaded_preview`, `node_count` and `last_error`; `sync_scene`, `upload_scene`,
  `viewport` and the `apply_*` handlers move onto it. `FurniCraftApp` stays the shell: the tabs, the
  settings and store, Settings/Home, the palette, the panel folds, the viewport cluster, the transform
  mode and the pointer. Every tab registers its own `TextureId` from its own `TextureView` and a close
  calls `Renderer::free_texture` for that id; the last close leaves one empty *Untitled*, never zero.
  `header.rs` gains the tab row and the `ActivateTab` / `NewTab` / `CloseTab` actions, and the dirty
  close reuses the `FileMenu` confirm dialog. Owner decisions: *Open* and *New* open a new tab and
  re-opening a path already open activates that tab instead of a second copy; the split layout travels
  with the tab because it already owns the cameras. Four commits: (1) `tabs.rs` and its tests with no
  behaviour change; (2) extract `Document` at one tab, all tests green; (3) the tab row, activation,
  close and per-tab textures; (4) ADR 0079, the map row, the plan close and the gate.
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src/file_menu.rs`

- `[ ]` **U48** the tree gains the eye and the padlock: every row of the scene tree carries a
  visibility toggle and a lock toggle, drawn at 16 pt and revealed on hover or when the flag is set,
  and both are session state keyed by node `Id` — no command, no schema, no undo entry (ADR 0065).
  Hiding a row hides its whole subtree from the viewport: the scene builder skips a hidden node, and
  picking skips it too, so a hidden node cannot be selected by a click while the tree can still show
  it as hidden and restore it. Locking a row keeps it drawn and selectable but refuses a gizmo drag
  and a destructive command, and the status bar says which node is locked; the refusal happens in the
  shell before a dispatch, because it is a guard on a gesture rather than a document rule. Both sets
  are pruned like the open containers, so a deleted or undone node never leaves a stale flag, and an
  *Isolate* / *Show all* pair in the tree's context menu covers the common case of focusing on one
  assembly. Tests: hiding a container hides its children from the frame but not from the tree;
  picking returns nothing for a hidden node; a locked node still selects and still resists a drag and
  a delete; a deleted node's flags are dropped and an undone deletion starts visible and unlocked;
  the flags survive a selection change and die with the session.
  Session notes: `Editor` owns `open: HashSet<Id>` in `crates/ui/src/lib.rs` and prunes it in
  `retain_open`, which is the shape to copy for `hidden` and `locked`; `scene_build::evaluate` in
  `apps/furnicraft/src/scene_build.rs` is where a skipped subtree disappears, and
  `mesh_pick::pick_scene` plus `scene_gizmo::pick` are where picking must agree. `icons::actions`
  already declares `VISIBLE`, `HIDDEN`, `LOCK`, `UNLOCK` as zero-byte placeholders, so the buttons
  draw their text label until U46 fills them.
  `files: crates/ui/src/scene_tree.rs, apps/furnicraft/src/scene_build.rs`

- `[ ]` **U46** the icon set catches up with the new surfaces: the header, the ribbon groups, the
  viewport bar and the split layout get their drawings under `assets/icons`, so no chrome surface
  is left as a text fallback. New: `view/orbit`, `view/pan`, `view/zoom`, `view/layout`,
  `view/shaded-edges`, `view/wireframe`, `view/xray`, `view/translucent`, `view/sliders`,
  `view/front`, `view/top`, `view/right`, `view/axon`, `actions/command` for the palette, plus the
  remaining `actions` files the header and the settings use. Each follows `STYLE_GUIDE.md` and `assets/README.md`, gets its README row, its
  button and the existing footprint/coverage test, and a shared drawing stays two files per theme.
  The tree's four `actions` toggles (`visible`, `hidden`, `lock`, `unlock`) are part of this task and
  are needed by U48.
  A placeholder that is still empty keeps the text fallback, so this task can land in slices without breaking a surface.
  Session notes: the `window` theme and `actions/{file-save,undo,redo,settings}` are drawn;
  `assets/README.md` lists both. `crates/ui/src/icons.rs` guards every non-empty file with the 24-grid,
  footprint and coverage tests; `Theme::ALL` and the uri count (`56`) move together with a new icon.
  `files: assets/icons, crates/ui/src/icons.rs`

- `[ ]` **U49** the interface speaks English and Russian: one typed message table behind a language
  switch, and no user-facing string left inline. A new `crates/text` (L0, no dependencies, registered
  in the layer map and `PROJECT.md` in the same change) holds `Lang::{En, Ru}` and one `Message`
  variant per string; `Message::text(self, lang) -> &'static str` reads a `&[&str; Lang::COUNT]`
  table, so adding a third language changes `Lang` and turns every missing translation into a
  compile error instead of an English fallback. Every call site in `crates/ui` and
  `apps/furnicraft` draws through that table: the header, the ribbon tabs, group captions and button
  captions, the scene tree context menu, all inspector cards and section titles, the status bar,
  the file and modal dialogs, and the argument messages of `PanelDiagnostic`
  (`crates/ui/src/panel_card.rs` `diagnostic_message` moves into the table and stops being a
  `String` builder where the variant carries no argument). `CommandSpec::label` becomes a
  `Message` too — all 48 specs — so the palette (U47) and any future CLI or assistant print the
  same words; the `id` stays the identity and is never translated. Russian needs real plurals, so
  a counting message is a constructor rather than one format string: `Message::more(n)` and the
  like pick one/few/many themselves. The language is workspace settings, never document data: it is
  chosen in the U41 *Settings* window, applies at once, and a saved `.fcs` never carries it. A name
  the editor generates on its own — `{} body`, `{} drawing` — is fixed in the language that created
  it, and switching the language never renames a node; that rule goes into ADR 0067 with the rest.
  Not translated: command ids, `FurnType` numbers, material and node names, `mm`, and the
  `thiserror` messages of `model`, `units` and `project`, which stay English for logs and tests and
  are mapped to a `Message` at the UI boundary instead of being shown raw. Tests: every `Message`
  variant is non-empty in both languages; a missing language cannot compile; every command spec
  label resolves in both languages; switching the language redraws the header, the ribbon and a
  card without touching the project revision or the undo stack; a scene saved in Russian loads in
  English with its node names unchanged; the longest Russian ribbon caption fits the 48 pt labelled
  button of U39 without clipping. Session notes: `Lang` is a plain enum and the table is
  `const`, so `crates/text` needs no serde, no `egui` and no file I/O — it is data, like
  `furnicraft-units`. The 48 labels are the `CommandSpec::new` calls in
  `crates/engine/src/commands.rs`; `label()` is read in `crates/ui/src/toolbars.rs`,
  `crates/ui/src/lib.rs` and `apps/furnicraft/src/header.rs`. Existing inline strings are ~530 in
  `crates/ui` and ~200 in `apps/furnicraft`; `theme::PRODUCT_NAME` and the logo stay as they are.
  Do this after U39–U48 so the labels stop moving, and before M8/M9, which add hardware, cutting
  lists and CNC text on top of the same surface. A future move to per-language files (a translator
  working outside the repository) reuses `Message` as the key, so this does not lock the format.
  `files: crates/ui/src, apps/furnicraft/src, xtask/src/main.rs`

- `[ ]` **U50** the panel header loses the count and gains a scene-side fold: the digit next to
  *Scene* and *Inspector* disappears — the tree and the card already say what they hold, and the
  number cost a document scan on every rebuild — and the fold chevron moves to the edge facing the
  viewport, so the left panel folds from its right and the right panel from its left. The collapsed
  strip keeps its icon and turned title, and the click, edge-double-click and outward-drag restore
  rules from U40 stay. Tests: neither header draws a count; a left panel's chevron sits at its right
  edge and a right panel's at its left; folding and restoring keeps the expanded width.
  Session notes: `panel_header` and `count_chip` in `apps/furnicraft/src/main.rs` draw the current
  row; `node_count` is cached only for that chip. `PanelSide` already names which edge a panel sits
  on, so the chevron side follows from it.
  `files: apps/furnicraft/src/main.rs, crates/ui/src/theme.rs`

- `[ ]` **U51** the toolbars float over the design space: the ribbon and every other tool strip
  become overlay areas over the viewport instead of top panels that take height from it, so the scene
  image runs under them, a translucent ribbon blends with the furniture instead of the window
  background, and resizing a side panel never reflows or jumps the strip. The ribbon keeps its own
  clicks and scroll; the viewport keeps orbit, pan and zoom outside it. The viewport's fit and its
  picking rectangle account for the covered area, so a framed scene is not hidden under the ribbon.
  ADR needed.
  Tests: a ribbon with alpha below 1 over a frame samples the scene, not the chrome; resizing a side
  panel leaves the strip's anchor and width unchanged; a click inside the ribbon does not reach the
  viewport; frame-all keeps the bounds inside the uncovered rectangle.
  Session notes: `ribbon()` currently builds an `egui::Panel::top`; the view cluster already proves
  the `Order::Middle` overlay path (`view_cluster::cluster_anchor`). The render target must cover the
  full central rect; the pick rectangle is `viewport()`'s `available_rect_before_wrap`.
  `files: apps/furnicraft/src/main.rs, crates/ui/src/toolbars.rs, crates/ui/src/theme.rs`

- `[ ]` **U52** the UI scale is a scrubbable number with presets: the Settings slider becomes a numeric
  field that changes while the pointer is held and moved left or right, with the cursor hidden for
  the duration and only the horizontal delta read, plus buttons for 75, 100, 125, 150, 175 and 200.
  The same *hide the pointer, read the delta* rule applies to every numeric field that scrubs on a
  horizontal drag, so a value that resizes its own widget — the scale is the worst case — no longer
  chases the cursor. A typed value is clamped to the offered range and applied through
  `ctx.set_zoom_factor`; a preset applies exactly and draws as selected. Tests: a drag reads the
  delta and never the absolute pointer position; the value does not jump when the widget changes
  size under it; a preset sets its exact percent; a typed value outside the range clamps.
  Session notes: `ui_scale::show` is the only scale control; `Response::drag_delta` or
  `ui.input(|i| i.pointer.delta())` is the input to read, and `CursorIcon::None` hides the pointer.
  Find every scrubbing numeric field through `egui::DragValue` in `crates/ui/src`.
  `files: crates/ui/src/ui_scale.rs, crates/ui/src/settings.rs, crates/ui/src`

## Bugs

- `[ ]` **X8** the scene stays dark in the light theme: with the workspace on *Light*, the viewport
  still clears to the dark chrome, the floor grid keeps its light-on-dark lines, and the rendered
  frame reads as a dark hole in a light window. Expectation: a theme change repaints the scene
  background, the grid and the non-material lines to the active variant at once, leaving the
  furniture's own material colours untouched, and switching back restores the dark values.
  Suspected cause: the frame background and the grid line colours are literals in `crates/scene`,
  and the shipped frame never gets them from the active `egui::Visuals` when `theme::install_with`
  changes. Tests: a light frame carries the light background and grid; switching variant changes the
  uploaded scene without touching the document revision.
  `files: crates/scene/src/lib.rs, crates/render/src/lib.rs, apps/furnicraft/src/main.rs`

- `[ ]` **X22** a free corner drag on an arc makes a near-full circle: dragging the corner where an
  arc meets a line in *Free move* turns the arc into the long way round, and the draft draws a huge
  loop whose visible branch reads as a straight line through the joint and across the viewport.
  Expectation: a corner drag keeps the arc's class — a shallow arc stays shallow, a deliberate major
  arc stays major — and the draft never draws outside the figure. Suspected cause:
  `arc_with_radius` hands `arc_between` the side of the old arc's midpoint against the *new* chord,
  and `arc_between` returns the first candidate whose midpoint is on that side. For a shallow arc a
  1 mm corner drag rotates the new chord enough to move the old midpoint to the other side, so the
  test matches the major arc's midpoint and the sweep becomes ~360°. Reproduced with chord 100 mm,
  sagitta 0.5 mm (R 2500 mm, sweep -2.292°): a 1 mm corner drag yields sweep 357.731° and a sampled
  chain spanning x -2425..2575, y -4899..101 mm; the joints still meet, so the ring validates and
  Apply stores it. Introduced by X13 (`64c11d3`). Fix: choose the candidate by its sweep magnitude
  against the old arc's — nearest |sweep|, tie broken by the old sign — and pass the old sweep into
  `arc_between`; the centre side stays as it is. Tests: a shallow arc keeps a small sweep and its
  points stay inside the figure; a major arc stays major; the three existing `a_free_arc_end_*`
  tests stay green; a grip drag on a shallow arc keeps its class.
  `files: crates/model/src/contour_edit.rs, crates/ui/src/contour_grips.rs`

## Next

- `[ ]` **R1** the pre-release rename: the product name leaves the code before the first release, so a
  later choice of name is one constant and no refactor. The work is measured and scripted in
  `plan/rename.md`; read that file, not the tree. Four parts, each its own commit: (1) give the
  identity one home — `theme::PRODUCT_NAME` for the words, an `APP_ID` for the window class and the
  settings directory set through `ViewportBuilder::with_app_id`, `project::FORMAT` +
  `project::EXTENSION` for the file — and delete the hardcoded duplicates in `main.rs` (title, app
  name, error), `scene_build.rs` (`"FurniCraft sample"`), `file_menu.rs` (the `scene.fcs` default and
  the `"fcs"` filter) and `settings_store.rs` (three platform branches); (2) rename the 13 crates to
  bare names and move `apps/furnicraft` to `app/` at the workspace root, with the `sed` and `mv` in
  `plan/rename.md` and `cargo fmt --all` after them; (3) the four registers that must agree in that
  commit — the `CRATES` table and its tests in `xtask`, the crate map in `AGENTS.md` and `PROJECT.md`,
  the layer list in `skills/enforce-architecture/SKILL.md`, and the 38 path hints in
  `plan/BACKLOG.md`, `plan/map.md` and `plan/debt.md`, which the plan gate rejects once the path is
  gone; (4) the envelope `format`
  string and line 2 of the eight goldens, leaving the `generator` fixture alone. No compatibility
  path: nothing is released, so the file format and the settings directory change outright. Measured
  on a scratch copy of this tree: 1140 tests pass, clippy clean, `layers: ok`, 960 changed lines and
  no behaviour change. Test: one asserting that the window title, `app_id`, settings directory and
  open-dialog filter all read the constants, so a second rename cannot miss a copy.
  `files: plan/rename.md, crates/ui/src/theme.rs, crates/project/src/lib.rs`

## Done

- `[x]` **X20** a pole view has a canonical up: clicking the top view inherited whatever yaw the
  camera had, so the elevation arrived rolled. `Camera::look_from_oriented(direction, up)` takes the
  roll from a world up when the direction is a pole and keeps the existing yaw rule otherwise, and
  `canonical_up` is `+Y` for the top and bottom views and `+Z` for the rest. The widget's
  `GizmoAction::LookFrom`, `Leaf::apply_preset`, `Leaf::look_from`, `Leaf::frame` and
  `opening_camera` use it, so a top view looks down with `+Y` up and `+X` right, a bottom view looks
  up with `+Y` up and `-X` right, and the side and front views keep `+Z` up; the target, the distance
  and the projection do not move. ADR 0057 records the snap. Tests: an arbitrary yaw still lands on
  the canonical basis at both poles; a side view ignores the requested up; a pole with an up
  parallel to the view keeps its yaw; a preset and the widget produce the same camera.
  `b6a974f`
  `files: crates/scene/src/lib.rs, apps/furnicraft/src/view_layout.rs`

- `[x]` **X19** the extrusion pulls a phantom body: the arrow detached from the body while the pull
  was open and the body only appeared after release. The pull now draws a wireframe prism at the
  preview height — the sampled contour at the base and at the preview, joined by a vertical edge at
  every sampled corner — and the arrow sits on its top face, so the preview moves with the pointer
  while the document keeps the last committed height. `Body` keeps the contour's sampled points,
  `ghost_edges` builds the prism, `paint` draws it before the arrow in a translucent accent, and
  Escape drops the pull without touching the body. Tests: the ghost top is at the preview height
  and the base at zero; a drag changes the ghost with no revision or undo entry; release writes one
  height command and one undo restores it; Escape keeps the committed height. A solid preview is
  recorded in `plan/debt.md`.
  `efb153c`
  `files: apps/furnicraft/src/extrude_gizmo.rs`

- `[x]` **X18** a figure's geometry is editable in its card: the object card had no radius for a
  circle, no width and height for a rectangle and no side count for a polygon, and a constructive
  body's drawn path was not in the summary at all. The card now shows the fields the figure has —
  circle radius, rectangle width and height, polygon sides (`3..=128`) and radius, a single arc's
  radius and end angle — for a `Path` node and for the constructive body that owns a path, and Apply
  writes one command with one undo entry. `Constructive::path()` already existed; the summary now
  carries it, and a *Figure* section reads the fields through
  `circle_frame`/`rectangle_frame`/`polygon_frame`/`arc_frame` and writes them through the existing
  model setters. The edit dispatches `object.contour.replace` for a body and `object.path.replace`
  for a path node, both of which store the `Path` itself, so the shape tag and the segment ids
  survive; the fields are disabled while the viewport owns the working copy. Tests: each field
  reshapes the path and keeps its tag; a constructive body keeps its tag; a card edit during a
  viewport edit is refused; one undo restores the figure.
  `073fdb6`
  `files: crates/ui/src/object_card.rs, crates/model/src/object.rs, crates/engine/src/object_cmds.rs`

- `[x]` **X17** along-contour movement has a length, a stop and a collapse: the mode shows no length
  field, the node can pass the opposite node, and a zero-length segment cannot be finished.
  Expectation: a length field shows the distance from the segment's opposite end and accepts a
  typed length; the node cannot pass the opposite node; at 0 the corner reaches it, and Apply
  removes the zero-length segment and joins the two nodes; a ring keeps at least three segments.
  Suspected cause: `numeric_entry` opens only when `grip_number` returns a number, and an untagged
  polyline corner has none, while an arc corner numbers an angle or a radius; nothing clamps at the
  opposite node and nothing records a collapse. Fix: build the entry from the drag motion (`Length`,
  label `L`, value from the other end, Enter moves the corner that far along the support, arc length
  for an arc); clamp to `0..=segment_length`, record `collapse` on `ContourEdit`, remove collapsed
  segments in `apply`, refuse a ring below three, and keep one undo entry. Tests: a typed length
  moves the corner; a drag cannot pass the opposite node; reaching 0 collapses and Apply joins the
  nodes; a ring of three refuses; the undo entry is one.
  `fc2d1f3`
  `files: apps/furnicraft/src/contour_gizmo.rs, crates/model/src/contour_edit.rs, crates/ui/src/contour_edit.rs`

- `[x]` **X16** a rectangle and a polygon keep their figure: dragging a side midpoint collapses a
  rectangle to a line, a rotated rectangle turns into a parallelogram and a corner drag then yields
  a width of thousands of millimetres. Expectation: a midpoint drag changes one extent and never
  makes it zero, a rectangle keeps right angles and its axes at any angle, a corner drag stays in
  the document range, and a figure edited along a side stops being that figure and becomes a plain
  polyline. Owner decision: a midpoint crossing the opposite side clamps at a minimum extent of
  1 mm instead of mirroring. Suspected cause: `move_rectangle_midpoint` refuses a non-positive half
  but leaves a sliver; `rectangle_frame` never orthogonalises the axes and
  `PathShape::fits(Rectangle)` only counts four lines; `move_rectangle_corner` does not clamp;
  `rebuild` keeps the tag through an along-contour edit. Fix: orthonormalise the axes and require
  right angles within a micrometre, clamp the midpoint at the minimum, clamp the corner, and drop
  the tag on a topology change. Tests: a rotated rectangle keeps right angles through a midpoint
  drag; a midpoint drag never yields zero; a corner drag cannot leave the range; an along-contour
  edit drops the tag; a tagged rectangle that is not right-angled is a load error.
  `bf07643`
  `files: crates/model/src/contour_edit.rs, crates/model/src/contour.rs, crates/ui/src/contour_grips.rs`

- `[x]` **X14** the grip set matches the figure: a regular polygon shows a phantom node outside its
  first side whose drag cannot be finished, and dragging a circle's rim deforms one quarter until
  the path is invalid. Expectation: a polygon shows its vertices and centre only and takes its
  radius from a vertex and from the card; a circle can be pulled from any point on its rim in any
  mode and only its radius changes; no grip sits off the figure. Suspected cause:
  `PolygonFrame::radius_point` sits on the circumcircle midway between the first two vertices,
  outside the first side; a tagged circle is four quarter arcs, and `grip_at` offers the synthetic
  `ArcRadius` on any hovered arc, which deforms one quarter. Fix: drop
  `ContourGrip::PolygonRadius` from the grip list and its number/value/label/drag/set tables; add a
  circle-rim grip for a `PathShape::Circle` draft that calls `set_circle_radius_at` and numbers `R`;
  an untagged arc chain keeps `ArcPoint`/`ArcRadius`. Tests: a circle pulled at 37° stays round and
  takes the radius from the centre; a polygon has no grip off the figure; a vertex still scales it;
  an arc chain keeps its arc grips.
  `916b15e`
  `files: crates/ui/src/contour_grips.rs, crates/model/src/contour_edit.rs, apps/furnicraft/src/contour_gizmo.rs`

- `[x]` **X15** the pointer grabs the node it is on: a press that visually lands on a grip often does
  not grab it, and the cursor turns into a hand over a node. Expectation: a press within the grip's
  hit radius grabs it even when the pointer moves a few points before egui's drag threshold, and
  the cursor stays the default arrow. Suspected cause: `ContourGizmo::update` resolves the grip from
  the current pointer when `drag_started` fires, which is after the threshold, and `cursor()`
  returns `Grab`/`Grabbing`/`PointingHand`. Fix: resolve from `pointer.press_origin()`, keep the
  grabbed grip for the whole drag, keep the 9–12 pt hit radius and let the enlarged hovered grip
  carry the feedback; `cursor()` returns `Default` and the shell's contour branch goes. Tests: a
  press exactly on a grip grabs it; a press 12 pt away does not; a press followed by a 5 pt move
  before the threshold still grabs; the cursor is the default.
  `52f1d0f`
  `files: apps/furnicraft/src/contour_gizmo.rs, apps/furnicraft/src/main.rs`

- `[x]` **X13** an arc follows its nodes: an arc cannot be reshaped by dragging its nodes — the
  endpoints only slide along the arc and change its angle — and dragging the middle node makes the
  radius jump to an absurd value. Expectation: every node follows the pointer; a free endpoint
  rebuilds the adjoining arc through its other end and its current mid point, the mid node rebuilds
  it through both endpoints and the new mid point, the centre still moves the whole arc, and
  sliding along the arc stays an explicit mode. Suspected cause: `ContourGizmo::dragged_point`
  projects every free corner onto an adjoining arc's circle, and the `ArcPoint`/`ArcRadius` drags
  call `Path::set_arc_radius_at`, which takes the radius from the pointer's distance to the centre.
  Fix: project only for `CornerMove::Along`; rebuild in `Path::free_corner` and a new
  `set_arc_through_midpoint`; refuse a degenerate rebuild, keep the last valid arc and cap the
  radius at the document range. Tests: a free arc end follows the pointer and changes the radius;
  the other end stays; the mid node stays finite; a collinear mid point is refused; the along mode
  keeps centre and radius. ADR 0075 is updated.
  `64c11d3`
  `files: crates/model/src/contour_edit.rs, apps/furnicraft/src/contour_gizmo.rs`

- `[x]` **X12** a drawing tool can be switched and cancelled: choosing another drawing tool while one
  runs does nothing, Escape does not reset the tool, and only Ctrl+Esc discards. Expectation:
  choosing another tool ends the running one and starts the new one, Escape cancels the draft and
  leaves the tool with nothing written, Enter and the right button commit a valid draft, the
  drawing shortcuts work while a draft runs, and the status line names the keys. Owner decision: a
  tool switch discards the running draft, because nothing was written and undo gains no half
  figure. Suspected cause: `Editor::start_drawing_with` refuses while `drawing.is_some()`, the shell
  skips the shortcut table while a draft runs, and Escape and Enter both commit (U53/ADR 0076).
  Fix: replace the draft instead of refusing, route Escape to `cancel_drawing`, keep Enter and the
  right button on commit, and let a failed commit keep the draft and the tool. Tests: start a
  polyline, press R, the rectangle tool is live with an empty draft; Escape leaves the tool and the
  document untouched; Enter commits; a shortcut starts a tool while a draft runs; a failed commit
  keeps it. ADR 0076 is updated.
  `62cfc45`
  `files: crates/ui/src/lib.rs, apps/furnicraft/src/main.rs, apps/furnicraft/src/draw_gizmo.rs`

- `[x]` **X21** a polyline closes through the arc it is drawing: with arc mode on and the arc's start
  and middle already placed, the *Close* button refused with "the arc is still waiting for its end
  point", so the ring could not be finished by that arc, and the first node was not highlighted as
  the closing joint while the arc waited. Expectation: *Close* completes the pending arc from the
  chain's stored end through its middle point to the first start and closes the ring, both close
  buttons enable from two segments on — the count a closing segment already needs — and the first
  node turns green when a click there would close the chain, in arc mode too. Suspected cause:
  `DrawDraft::close` refused while `pending_arc.is_some()`, `DrawDraft::can_close` demanded
  `pending_arc.is_none()` so the green first-point highlight and the ribbon's *Close* were off, and
  `draft_can_close` plus the status bar demanded three segments. Fix: `closing_segment` builds the
  pending arc as the closing segment through the first start and `close`/`to_path` share it,
  `can_close` counts the pending arc, both close buttons enable from two segments through
  `draft_can_close`, and `closes_at` lights the first node. Undo reopens a closed chain before it
  takes the waiting arc; `push` refuses a closed draft first. Tests: a two-line chain with a pending
  arc closes and reaches a contour; its closing segment is the arc; a degenerate closing arc is
  refused and leaves the draft editable; undo keeps the closing arc; the first node lights up with a
  pending arc; both Close buttons enable from two segments; the gesture commits one closed path and
  one undo entry. ADR 0077.
  `8ff248f`
  `files: crates/ui/src/contour_edit.rs, apps/furnicraft/src/draw_gizmo.rs, crates/ui/src/toolbars.rs, apps/furnicraft/src/main.rs`

- `[x]` **X11** a polyline closed by an arc is one ring: a chain whose last segment is an arc never
  closes — the third click on the first point does not finish it and the command reports "path
  segment 2 does not meet the next segment" — and the arc preview blinks at an intermediate click.
  Expectation: an arc's derived endpoints join their neighbours within the drawing tolerance, a
  closing click in arc mode ends the arc on the first point and closes the ring, and the preview
  keeps the pending arc and never blanks. Suspected cause: the joint tolerance is a constant
  micrometre (ADR 0072), while an arc endpoint is derived from a rounded centre, radius and angle
  and misses by `radius × 8.73e-6` — about 5 µm on a 600 mm arc; `DrawDraft::finish_arc` sets
  `closed` without validating the ring, and `preview` clears the cache when it returns `Ok(None)`.
  Fix: a pair rule, `1 µm + arc_slack(first) + arc_slack(next)` with `arc_slack = radius × 8.73e-6`
  (zero for a line), used by `Path::assemble`, `Contour::assemble` and `to_geometry`; `finish_arc`
  closes through `close()`; `preview` keeps the last valid path and paints the pending arc.
  Tests: a 600 mm arc closes a polyline and reaches a contour; the same ring extrudes; a 20 µm gap
  on a 100 mm arc is still refused; a rejected preview keeps the last valid one; a 1 µm gap loads.
  Correct ADR 0072; note explicit arc endpoints (schema 20) in `plan/debt.md`.
  `f3123d7`
  `files: crates/model/src/contour.rs, crates/ui/src/contour_edit.rs, apps/furnicraft/src/draw_gizmo.rs`

- `[x]` **U55** a fresh extrusion pulls its height in the viewport: the ribbon creates the body at
  the last height used in the session (100 mm at first) and reports it, so an arrow runs from the
  contour's bounds centre along the body's local `+Z`. Dragging it writes one
  `object.extrude.height.set` on release and keeps the pull open; a bare click or Enter finishes,
  Escape drops the pull at the height the document already holds, and a typed number applies the
  same way. The height is session view state until the command, never a document default. Tests: a
  drag maps to millimetres along the body's own axis, a click finishes at the pull's height, a
  pulled height becomes the next extrusion's default, and a path has no pull.
  `5efa195`
  `files: apps/furnicraft/src/main.rs`

- `[x]` **U54** every figure exposes its grips: a circle its centre and four quadrants, an arc its
  centre, a midpoint and a radius grip on a hovered curve, a rectangle its corners, four side
  midpoints and centre, a regular polygon its vertices, centre and radius point. A number field
  follows a grip drag and names its own dimension — a radius for a circle, arc or polygon, an angle
  for an arc end, `W` or `H` for a rectangle side; Enter applies it without leaving the edit and Esc
  restores the drag-start value. Free movement is the default, an HUD toggle switches to the segment
  the pointer picked at the drag start, and a tagged figure keeps its constraint through every drag.
  The polygon side count stays in the object card. Tests: a quadrant drag keeps the circle round; an
  arc centre moves the arc; a rectangle midpoint changes one extent; a polygon vertex keeps every
  side; a numeric entry parses its kind and applies it.
  ADR 0075 — `1321fee`
  `files: apps/furnicraft/src/contour_gizmo.rs`

- `[x]` **U53** the drawing tools that finish a shape: `DrawTool` gains arc from centre (three
  clicks), circle through three points, rectangle from its centre (two clicks), a directed
  rectangle as its own button (centre → side direction → corner, `Shift+D`) and a regular polygon
  (centre plus a rim point, `N` from the ribbon, `3..=128` sides). Tab switches polyline input
  between a straight segment and an arc and Alt ignores the end and grid snaps; the closing segment
  of a polyline is an arc when the closing click is taken in arc mode, and it always starts at the
  stored end. A preview that cannot become a path keeps the last valid preview and the raw chain
  stays drawn, so a drawing never blinks out at an intermediate click; the status line names the
  reason instead. Escape or the right button commits, Ctrl+Esc or Cancel discards, and a committed
  figure opens its own edit mode. Tests: Tab builds a straight then an arc segment; an arc closes
  the chain; a degenerate arc step is refused; each new tool builds and commits its path; an
  invalid intermediate preview keeps the previous one; a committed shape is in edit mode.
  ADR 0076 — `9778d35`
  `files: crates/ui/src/contour_edit.rs`

- `[x]` **A11** an extrusion owns the path it grew from: `Constructive` stores `{ path, features }`
  and `Extrude` keeps only its height, so the contour is no longer copied and "absorbed" means the
  body holds the one path. `object.path.extrude` turns the selected `Path` node into a constructive
  body in place (no sibling, no orphan), `object.contour.replace` edits the path inside the body and
  the mesh cache keys on it, and `object.extrude.height.set` drives the pull. Schema 19 migrates a
  version 18 `extrude.contour` into `constructive.path`. Tests: extruding a path replaces the node
  and leaves no second object; editing the contour inside the body rebuilds its mesh; the height
  command keeps the path; an old file loads into the new shape and saves without a contour copy.
  ADR 0074 — `c98f590`
  `files: crates/model/src/object.rs`

- `[x]` **A10** a corner moves freely or along the segment picked at drag start: `move_corner_at`
  takes a `CornerMove` (`Free` or `AlongContour`) plus the target resolved once by the caller from
  the initial drag vector, so a junction does not change its mind mid-drag. A free arc endpoint takes
  the point and rebuilds the arc through the other end and its current mid-arc point, so the radius
  follows the drag and the arc can be shortened; a degenerate step is refused and the grip stays on
  the last valid arc. Numeric grip values land in the document layer: arc radius and end angle,
  circle radius, rectangle width and height, polygon radius. Tests: a free arc end changes the
  radius; the arc can be shortened to its chord; a degenerate target is refused; the along-contour
  target is the segment chosen at the start; each numeric value keeps the figure's own constraint.
  `9553175`
  `files: crates/model/src/contour_edit.rs`

- `[x]` **A9** a path remembers the shape it was drawn as: `Path` gains an optional `PathShape`
  (`Rectangle`, `Circle`, `RegularPolygon`) at schema 19, an edit-intent tag rather than stored
  dimensions — the centre, radius and side count are derived from the segments, the tag only says
  which constraint set the grips apply. Structural constructors land with it: arc from centre
  (centre, start, end), circle through three points, rectangle from its centre, rectangle from
  centre plus a side direction, and a regular polygon from centre, rim point and `3..=128` sides.
  A topology change (split, join, dropped segment) clears the tag; a tag that contradicts the
  geometry is a readable load error, not a silent reinterpretation. Tests: each constructor sets its
  tag and its geometry; a rectangle edited by a corner keeps the tag; a split clears it; a version 18
  file loads without a tag; an unknown tag is an error.
  ADR 0073 — `9d7476f`
  `files: crates/model/src/contour.rs`

- `[x]` **A8** one joint tolerance for a ring: `Contour::assemble` compares a joint with the same
  one-micrometre bound `Path` already uses instead of demanding exact equality. An arc stores its
  centre, radius and angles, so its endpoints are derived and can miss the drawn point by a
  micrometre after rounding, while `Path` accepted the joint and `Contour` refused it: a polyline
  closed with an arc failed with "path segment 2 does not meet the next segment", the arc-arc joint
  recorded in `plan/debt.md` failed the same way, and `DrawingDraft::closing_segment` could only add
  a straight line. One rule, stated in ADR 0072, covers the path, the ring and `to_geometry`.
  Tests: a ring whose arc endpoint misses the next start by one micrometre loads and converts to
  geometry; two micrometres still fail; an arc that closes a polyline builds.
  ADR 0072 — `a00cb6f`
  `files: crates/model/src/contour.rs`

- `[x]` **U44** a ground shadow: every object that rests on the floor casts a soft contact shadow, so
  a cabinet reads as standing on something instead of floating in a grid. `ContactShadow` is frame
  data in `crates/scene` beside `Grid`: a node `Id`, a centre on the ground plane and two half-axes
  derived from the object's world bounds, covering 92 % of its footprint so the ellipse stays inside
  the outline. One shadow per root node whose solid subtree touches the floor, with a 0.5 mm
  tolerance; a path or drawing on the work plane casts nothing. The renderer draws one unlit
  transparent triangle fan per shadow with a radial falloff in the shader — no shadow map, no extra
  pass, no texture — on a plane just below the grid, and the switch and strength travel in the camera
  uniform, so toggling uploads nothing. A checkbox and the strength slider sit in the display-settings
  popover. Tests: a bound with no floor contact draws nothing; the ellipse stays inside the footprint;
  the switch empties the pass; the sample scene emits exactly one shadow. ADR 0071 — `4e0348f`
  `files: crates/scene/src/lib.rs, crates/render/src/lib.rs`

- `[x]` **X10** every triangle edge is drawn, and far edges show through near panels: a flat panel
  draws the diagonals of its triangulation instead of its outline, and a panel a metre behind another
  shows its edges through it in *shaded with edges* and *wireframe*. A dark viewport draws dark ink,
  so edges vanish on the default background. Expectation: the default edge pass draws form edges only
  — a boundary or a real crease, so a box shows twelve, not eighteen, and a triangular body nine —
  the full triangulation is a switch in the display settings, the ink is light on a dark viewport,
  and an edge behind a face is not drawn unless the *hidden edges* style is on, which shows one
  object's own inside but never the object behind it.
  Suspected cause: `crates/mesh` deduplicated edges by position but kept every one, so coplanar
  diagonals survived; the `vs_edge` shader moved clip-space depth by a constant, which is ~90 mm of
  world offset at one metre and >1 m at five, so a far line overtook the near face in front of it;
  `edge_color` was a single dark constant; and the hidden pass compared depth with `Always` and
  nothing else, so it drew through every object.
  Tests: a coplanar quad drops its diagonal and keeps four boundary edges; a 90° fold keeps its
  shared edge; a cube yields twelve edges and eighteen in the diagnostic list; a triangular prism
  yields nine; an occluded drawable's edge slice is separate from the near one and carries its own
  stencil slot; the face pass owns the depth bias and stamps the slot; the hidden pass accepts only
  its own slot; the ink follows the background. ADR 0070 — `afeba77`
  `files: crates/mesh/src/lib.rs, crates/render/src/lib.rs, crates/scene/src/lib.rs`

- `[x]` **U43** the display styles the bar offers become real: *shaded with edges*, *shaded*,
  *wireframe*, *shaded with hidden edges* and *translucent*. Edges are derived data — one deduplicated
  edge list per mesh, cached in `crates/mesh` against the same derivation inputs as the mesh itself —
  and drawn as a line pass with a depth bias, because `PolygonMode::Line` is unavailable on Metal and
  a 1 px line cannot be widened (ADR 0059). *Shaded with hidden edges* draws the edge pass with
  `CompareFunction::Always`; *translucent* draws with blending and no depth write and orders objects
  by their centre, which is honest for a cabinet of flat panels; *wireframe* draws the edge pass
  alone. `ShadingStyle` lives in `crates/scene` as frame data and the renderer interprets it, so a
  test can build a frame without a GPU. The bar's buttons and the settings default stay in step, and
  a style change re-uploads nothing but the style. Tests: an edge list deduplicates a shared edge and
  keeps a boundary edge; each style maps to the pipeline state it promises; a translucent pass writes
  no depth; a wireframe frame draws no triangle. ADR 0070 — `fc585cc`
  `files: crates/mesh/src/lib.rs, crates/render/src/lib.rs, crates/scene/src/lib.rs`

- `[x]` **X9** a split viewport draws each camera in the wrong half: with two views stacked or four in
  a grid, orbiting, panning or picking in the top view moved the bottom one, and clicking an axis on
  the orientation widget in one view snapped its neighbour. Expectation: a view draws the camera its
  own rectangle belongs to, so a gesture and a widget click act on the view under the pointer, in
  both directions and in all four quadrants. Suspected cause: `ViewRegion::scissor` flipped the
  region's `y` as though the framebuffer grew upwards, while the pointer was tested against the
  top-left rectangles the shell paints; a one-view workspace hid it because the whole target is one
  region and a flip of it is the identity — `6690ae6`
  `files: crates/render/src/lib.rs, apps/furnicraft/src/main.rs`

- `[x]` **U42** the workspace divides into views: the central area holds a layout tree of leaves and
  splits — one view by default, then *two vertical*, *two horizontal* and *four* from the bar, and
  any border between two views is draggable with a minimum size of 120 pt per side and a ratio
  clamped to 0.15–0.85. Each leaf owns its camera and its preset (*Front*, *Top*, *Right*,
  *Axonometric*), *Home* and *Frame all* act on the view under the pointer or on the active one, and
  the active view carries a thin accent frame. The layout, the cameras and the active leaf are view
  state in the shell and never reach the document; a preset writes yaw and pitch only, so switching
  views never moves the model. The orientation widget gains a *home* button that returns the active
  leaf to *Axonometric*, and an optional readout under it — `Pitch 32.1° · Yaw -51.7° · Roll 0.0° ·
  Ortho` — drawn only when the switch U40 added to the display-settings popover is on; the numbers
  come from `Camera::pitch`/`yaw` and `Projection`, formatted through `furnicraft-units`, so no new
  document data and no second source of truth. The renderer draws one offscreen target and one view
  per pass with a scissored region and a uniform slot per view, rather than one texture per view, so
  memory does not grow with the grid and egui keeps a single texture id. Tests: a split and a join
  round-trip; a drag clamps at both ends and never collapses a view; a preset produces the expected
  orthographic basis; four views produce four scissor rectangles that tile the area exactly; the home
  button restores the axonometric preset; the readout is hidden when its switch is off and names
  `Ortho` or `Persp` in step with the projection.
  Session notes: `viewport()` owns the single target and calls `resize` +
  `update_egui_texture_from_wgpu_texture`. A split view needs a scissored pass per leaf into the same
  target (`set_scissor_rect`), and `ViewportRenderer` writes one `CameraUniform` per
  `render_with_lighting`, so a slot array with a dynamic offset keeps one buffer. Respect ADR 0049 per
  leaf: an authored camera pins its own projection. Reuse `scene_build::home_camera` for a leaf's fit —
  `502f870`
  `files: apps/furnicraft/src, crates/render/src/lib.rs, crates/scene/src/lib.rs`

- `[x]` **U47** a command palette on `Ctrl+K`: one search field over `CommandRegistry::specs()`,
  listing a command's label, its id and whether it is enabled for the current session; the chosen
  entry dispatches it with parameters built from the current selection, and an entry that cannot run
  says why instead of failing silently. The palette is the reference's "command search" and it costs
  no new command: the registry already carries 48 specs with a stable id, a label and a pure
  `enabled` check, so the work is a fuzzy filter, a keyboard loop (`↑`/`↓`/`Enter`/`Esc`) and one
  parameter builder per command family — node deletion, duplication, renaming and the view actions
  first, with the rest reported as "needs a selection" until a builder exists. Command history and
  the last-used list are session state, never document data. Tests: a prefix and a substring both
  find `node.delete`; a disabled command is listed but not runnable and explains itself; `Esc`
  closes without dispatching; a chosen entry dispatches exactly one command and one undo entry.
  Session notes: `CommandRegistry::specs()` returns `&'static [CommandSpec]`, `spec.id()` and
  `spec.label()` are the search fields and `spec.enabled(session, &params)` is the pure check;
  `crate::command::dispatch` in `crates/ui` is the one dispatch path. `Ctrl+K` is free — `F`, `O`,
  `G` and the drawing letters are taken, and `ui.input(|i| i.modifiers.command)` must not fire while
  a text field has focus.
  `8066ec8`
  `files: apps/furnicraft/src, crates/ui/src`

- `[x]` **U41** the settings window and the one place the workspace is remembered: a *Settings*
  modal with the sections *Appearance* (theme dark/light/system, accent, UI scale — the control U40
  left without a surface moves here), *Viewport* (default display style, grid on and its
  step, edges, the view a new viewport opens with), *Ribbon* (background opacity) and *Shortcuts*
  (the existing table). The window is a view, not a document surface: a change applies at once and
  never touches the `.fcs`, and a scene saved by another machine carries none of it. The state is
  written as JSON under the operating system's config directory through a small `settings` module
  with an atomic write, the same shape `furnicraft-project` uses, so a crash during a save cannot
  leave a half-written file; a missing, unreadable or older file falls back to the defaults with the
  reason in the status bar instead of a panic. The same module gains the *Home* surface that the
  header's house button opens: new, open, the recent files and the empty template, drawn over the
  workspace and dismissed by Escape. Tests: a round trip through JSON restores every field; a
  truncated or unknown-version file yields defaults; a value outside a range is clamped on read; the settings never appear in a serialised document.
  `c83ace5`, `bbef3e2`
  `files: apps/furnicraft/src, crates/ui/src`

- `[x]` **U40** the side panels become windows that fold and the viewport gets its floating cluster:
  the scene tree and the inspector each get a header row — a title, a count chip and a fold chevron —
  and folding them leaves a 26 pt vertical strip with the icon and the rotated title, which a click,
  a double-click on its edge or a drag outward restores. The tree keeps only the tree — no tools, no
  heading of its own — and still supports selection and the context menu. The view controls become a
  translucent cluster floating over the centre of the viewport, anchored `CENTER_BOTTOM` with a small
  offset: *orbit / pan / zoom* · *frame all / home* · the display-style buttons · the layout entry and
  the three-slider display settings popover, which also carries the *show projection and angles*
  switch that draws the pitch/yaw/roll and the projection word under the orientation cube (U42). The
  bottom status bar stays thin and holds only the armed tool, the gesture it expects, the counts and
  the last error, so the viewport starts at the scene and not at a wall of labels. A row of buttons is
  not foldable; only a panel with content is. Tests: folding and restoring a panel keeps its width;
  an armed container still names itself in the bar and clears from it; a closed panel draws no body;
  the status line names the armed tool and its gesture; the cluster is anchored to the bottom centre
  and does not move when the side panels resize; the settings popover toggles the angle readout —
  `4934ebf`
  `files: crates/ui/src/lib.rs, apps/furnicraft/src/main.rs`

- `[x]` **X7** a ribbon button is one measured cell, so a caption never sizes it: the tool row of
  every tab draws buttons of one width and one height, whatever their caption and whatever the
  window width, and a caption that does not fit is elided with `…` instead of widening the button.
  A placeholder asset keeps the same cell and draws its caption as the face, so a half-drawn icon
  set has one rhythm. The vertical divider between two groups spans the whole body — the tool row
  and the caption band — instead of the 16 pt header hairline, and the header's own row follows the
  same rule: `File` and the icon buttons are one 26 pt control, and its hairlines are as tall as they
  are. Observed: the Sketch row had buttons from 41 to 83 pt wide, Rectangle was the widest and the
  Arc tool the narrowest, the divider stopped in the middle of the row, and a maximised window
  stretched every caption into its own width. Expected: one cell, one pitch, one divider height, and
  captions that read as a column. The cell is measured once per row from the window and clamped to
  a minimum and a maximum — the owner's second observation — and the minimum is what the longest
  Russian caption needs, which is the U49 requirement. Tests: every caption of the ribbon yields the
  same cell width and height; a caption past the inner width is elided; the Russian tool captions fit
  the narrowest cell; a placeholder occupies the same cell as a drawn icon; a narrow window gives the
  floor and a maximised one the ceiling; the group divider is as tall as the tool row plus its caption
  band; every tab's column count is right. ADR 0067 — `7d4895c`
  `files: crates/ui/src/icons.rs, crates/ui/src/theme.rs, crates/ui/src/toolbars.rs`

- `[x]` **U39** the ribbon replaces the strip that lives inside the scene tree: one row of tabs
  (*Furniture, Hardware, Model, Sketch, Scene, View*) sits under the header, and the body under it
  draws the tool groups of the active tab separated by vertical dividers, heading each group
  underneath in 10.5 pt caps. A tab whose every group is empty is not drawn at all, so no greyed
  "coming soon" tool ever appears; the tab that is open is the accent, a collapsed ribbon keeps only
  its tab row, and clicking the active tab or double-clicking its label collapses and restores the
  body. The groups map onto the actions that already exist — Sketch: line, polyline, arc, rectangle,
  circle, close, cancel; Model: cube, cylinder, sphere, cone, extrude; Scene: panel, assembly,
  camera, light; View: frame all, projection, grid — and the numeric fields (primitive size,
  extrusion height) become a labelled field inside their group rather than a floating row. A button
  in the ribbon is a *labelled* button: a 22 pt icon over a 10.5 pt caption, on a 48 pt row, with the
  group's own 14 pt caption underneath, so the body is about 90 pt tall like the reference the owner
  showed. `theme::RIBBON_ROW_HEIGHT` grows to 48, `theme::RIBBON_CAPTION_HEIGHT` is added, and a new
  `icons::ribbon_button` draws icon-plus-caption; `icons::tool_button` stays the icon-only button for
  the header, the panel headers and the floating viewport cluster, because a caption there would
  double a 26 pt row. The armed drawing tool or primitive still draws pressed, and the creation chip
  moves to the status bar (U40). Everything is view state: no command, no schema, no undo entry.
  Tests: every implemented action appears in exactly one group and every group has at least one
  action; an empty tab is not drawn; collapsing keeps the tab row; the pressed tool and the disabled
  extrude hint survive the move; a labelled button paints both its icon and its caption.
  — `36a9763`

- `[x]` **U38** the window reads as one frame: one theme owns the colour and the rhythm, and chrome
  is separated from content. `crates/ui/src/theme.rs` holds the palette (chrome, surface, panel,
  control, line, text, dim, disabled, one accent, danger, warn), a 4 pt spacing step, row heights, one
  rounding and one type scale, and `install` applies the same style to both of `egui`'s variants so a
  system light preference cannot leave half the window in the stock palette. `header.rs` draws one row
  — mark, *FurniCraft*, *Home*, *File*, *Save*, *Undo*, *Redo*, the open tab with its unsaved dot, and
  the gear pinned to the far edge with `Sides` — and returns a `HeaderAction` the shell performs, so
  the row owns no document state; the window keeps its system frame, so no window buttons are drawn.
  The status text, the view controls, the work plane and the UI scale moved out of the viewport's first
  row into a bottom `status_bar`, drawn from a snapshot that applies its one action after both sides
  have drawn. The scene tree and the inspector lost their stock headings for a themed title and
  hairline. New assets: `assets/logo.svg`, the `window` theme (`minimize`, `maximize`, `restore`,
  `close`, drawn for a future frameless mode) and `actions/{file-save,undo,redo,settings}`; `view/home`.
  ADR 0063 (theme) and 0064 (header) — `c7ad880`
  `files: apps/furnicraft/src/main.rs, crates/ui/src/toolbars.rs`
- `[x]` **U29** an arc inside the polyline: the Polyline tool appends either a straight segment or a
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
  Shipped as one draft of segments with Alt+click arcs — `ad95254`
  `files: crates/ui/src/contour_edit.rs, apps/furnicraft/src/draw_gizmo.rs, crates/model/src/contour.rs`
- `[x]` **X2** a valid contour with more than 1024 sampled points cannot become an extrusion.
  Observed: the command now refuses it with "one extrusion cap holds at most 1024", where before
  A6 it was stored and drew nothing. Expected: any simple ring the user can draw becomes a solid.
  Cause: one extrusion cap is one document face, capped by `MAX_FACE_VERTICES`; the ring samples at
  50 µm, so a smooth ring of a few turns exceeds it while the ring itself is legal (at most 256
  segments). `object.collapse` fails on such an object and `object_mesh` cannot draw it. Fixed by ADR
  0062: the chord is 1e-4 of the shape's diagonal above a 0.05 mm floor, and a ring that still
  overflows is re-sampled coarser within a bounded search — `cb89f2e`
  `files: crates/param/src/profile.rs`
- `[x]` **U28** mass and one projection: `fill-opacity` is allowed (still no `defs`, `mask`,
  `clipPath`, gradient or CSS), so a passive face fills at 12–18 % and the result or changed geometry
  at 45–60 % under the heavy outline. The 18 drawn icons were redrawn inside the fixed `3..21`
  footprint at 20–45 % coverage on the one published axonometry; `STYLE_GUIDE.md` §5, §7.1, §7.3,
  §7.4, §9 and §15, `README.md`, `AUDIT.md`, the draw-icon skill and `crates/ui/src/icons.rs` carry
  the numbers, and a test reads footprint and coverage off the rasterised alpha. ADR 0055 — `42f6cde`
  `files: assets/icons/STYLE_GUIDE.md, crates/ui/src/icons.rs`

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
