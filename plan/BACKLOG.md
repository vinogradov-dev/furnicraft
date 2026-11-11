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

- `[ ]` **X22** a free corner drag on an arc makes a near-full circle: dragging the corner where an
  arc meets a line in *Free move* turns the arc into the long way round, and the draft draws a huge
  loop whose visible branch reads as a straight line through the joint and across the viewport.
  Expectation: a corner drag keeps the arc's class — a shallow arc stays shallow, a deliberate major
  arc stays major — and the draft never draws outside the figure. Suspected cause:
  `Arc::with_radius_through` picks the new centre on the side of the old arc's midpoint against the
  *new* chord and then returns the first direction whose midpoint is on that side. For a shallow arc
  a 1 mm corner drag rotates the new chord enough to move the old midpoint to the other side, so the
  test matches the major arc's midpoint and the sweep becomes ~360°. Reproduced with chord 100 mm,
  sagitta 0.5 mm (R 2500 mm, sweep -2.292°): a 1 mm corner drag yields sweep 357.731° and a sampled
  chain spanning x -2425..2575, y -4899..101 mm; the joints still meet, so the ring validates and
  Apply stores it. Introduced by X13 (`64c11d3`); S12 moved the maths, not the defect. Fix: choose
  the direction by its sweep magnitude against the old arc's — nearest |sweep|, tie broken by the
  old sign — and pass the old sweep into the rebuild; the centre side stays as it is. Tests: a
  shallow arc keeps a small sweep and its points stay inside the figure; a major arc stays major;
  the three existing `a_free_arc_end_*` tests stay green; a grip drag on a shallow arc keeps its
  class.
  `files: crates/geom/src/segment.rs, crates/ui/src/contour_grips.rs`

## Bugs

- `[ ]` **X31** an along-contour drag on a sub-millimetre segment panics: dragging a corner along a
  segment shorter than twice the clamp margin (2 µm) aborts the editor instead of clamping or
  refusing. Expectation: the corner stops at the segment's own ends, or the edit is refused with an
  error the user can read; a furniture project never crashes. Reproduced: a path with a 1 µm first
  segment and `move_corner_at(PathCorner::End(id 1), point(50, 0), CornerMove::Along(id 1))` panics
  in `f64::clamp` with `min = 1.0, max = 0.0`. Suspected cause: `Segment::clamp_to_extent` turns the
  1 µm margin into a parameter fraction by dividing it by the segment's length, so for a segment
  shorter than 2 µm the lower bound exceeds the upper; the document's `clamp_to_curve` computed the
  same fraction before S12, so the move inherited the defect unchanged. A 1 µm line is a legal
  document segment: `ContourSegment::line` refuses only a coincident pair. Tests: a drag along a
  1 µm line and along a 1 µm arc returns `Err` instead of panicking; a drag along a normal segment
  still stops one micrometre short of the far end.
  `files: crates/geom/src/segment.rs, crates/model/src/contour_edit.rs`

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

- `[x]` **X34** origin and pivot are separate concepts, and the code names, commands, card and schema must say so
  Why: origin (the local zero, named by Placement::translation) and pivot (the authored local
   offset, the Pivot field) are still one word in the creation API and in the panel card, so a
   general rebase can silently move a pivot and a panel's derived base corner is presented as
   an authored handle.
  Shape: P0 renames the origin vocabulary, Pivot to PivotLocal and the wire field to
   pivot_local with a serde alias; P1 makes rebase_origin a general operation that preserves
   both world vertices and pivot_world and adds origin_local/origin_world; P2 keeps a panel's
   pivot derived and zero; P3 splits the inspector into Origin and Pivot; P4 bumps the schema
   without moving the world; P5 writes ADR 0088 and corrects the decisions and domain docs.
  Done: every creation path names its origin, PivotLocal is the only pivot, rebase_origin
   preserves world vertices and pivot_world, set_pivot/center_pivot change only pivot_local,
   placement.world_zero changes only translation, a panel's pivot is read-only zero, old
   goldens load unchanged through the alias, ADR 0088 exists, cargo xtask ci is green.
  `files: crates/model/src/frame.rs, crates/model/src/placement.rs, crates/model/src/contour.rs, crates/model/src/project/object_cmds.rs, crates/model/src/project/node_cmds.rs, crates/model/src/project/panel_cmds.rs, crates/model/src/project/query.rs, crates/model/src/project/wire.rs, crates/model/src/project/frame_contract.rs, crates/engine/src/node_cmds.rs, crates/engine/src/session.rs, crates/ui/src/pivot_card.rs, crates/ui/src/placement_card.rs, crates/ui/src/coords.rs, crates/text/src/messages.rs, docs/decisions/0088-origin-and-pivot-are-different-points.md, docs/decisions/0026-the-manipulation-pivot-is-stored-node-data.md, docs/decisions/0082-the-local-zero-is-the-creation-anchor.md, docs/decisions/0083-a-panel-is-anchored-at-its-base-corner.md, docs/domain/document.md, docs/domain/geometry.md, docs/domain/panels.md, docs/domain/scene-file.md, crates/project/tests/golden/scene-v20.fcs, PROJECT.md, AGENTS.md, plan/map.md` — `c250d09`

- `[x]` **X8** the scene stays dark in the light theme: with the workspace on *Light*, the viewport
  still clears to the dark chrome, the floor grid keeps its light-on-dark lines, and the rendered
  frame reads as a dark hole in a light window. Expectation: a theme change repaints the scene
  background, the grid and the non-material lines to the active variant at once, leaving the
  furniture's own material colours untouched, and switching back restores the dark values.
  Suspected cause: the frame background and the grid line colours are literals in `crates/scene`,
  and the shipped frame never gets them from the active `egui::Visuals` when `theme::install_with`
  changes. Tests: a light frame carries the light background and grid; switching variant changes the
  uploaded scene without touching the document revision.
  `files: crates/scene/src/lib.rs, crates/render/src/lib.rs, apps/furnicraft/src/main.rs` — `eba2f51`

- `[x]` **X33** a drag on the UI scale makes the window jump and loses the scale
  Why: Dragging the UI-scale field makes the window jump between values: with the Settings
   window open on Appearance, pressing the scale field and moving the pointer slowly to the
   right scales the interface up, then throws it to 75 %, then up to 200 %, and the scale that
   is written to settings.json and restored on the next launch is whatever the oscillation
   ended on (75 % was seen). Measured: the drag is read as a point distance, and a scale change
   re-lays out every widget, so the same physical pointer position is a different number of
   points in the next pass — the drag adds its own rescaling as travel. Reproduced in a test
   that runs the real input pipeline (positions in points converted with the current scale,
   native density 1 and 2): the percentage ran 100, 101, 104, 105, 90, 87, 154, 171, 75, 200.
  Shape: The field measures the pointer's own motion, which no screen coordinate can disturb:
   the raw motion the platform reports when it has one (winit forwards its device motion as
   Event::MouseMoved), and otherwise the difference between two pointer places, each in the
   pixels of the scale it was measured at. The drag accumulates from the value the button went
   down on, including the trip egui counts before it calls a press a drag. The trips are kept
   so the number, and the percentage the control reports, are the ones the hand made.
  Done: Tests: a drag in the window's own pipeline climbs without ever falling back, with and
   without raw motion; the same physical travel on a 2x monitor moves the place-based path the
   same amount; the field tests still pass with a growing widget. Live: settings.json holding
   125 opens the window at 125 % with the 125 % preset selected, and a run leaves the file
   untouched.
  `files: crates/ui/src/scrub.rs, crates/ui/src/ui_scale.rs` — `7c01b95`

- `[x]` **U52** the UI scale is a scrubbable number with presets: the Settings slider becomes a numeric
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
  `files: crates/ui/src/ui_scale.rs, crates/ui/src/settings.rs, crates/ui/src` — `929b780`

- `[x]` **U50** the panel header loses the count and gains a scene-side fold: the digit next to
  *Scene* and *Inspector* disappears — the tree and the card already say what they hold, and the
  number cost a document scan on every rebuild — and the fold chevron moves to the edge facing the
  viewport, so the left panel folds from its right and the right panel from its left. The collapsed
  strip keeps its icon and turned title, and the click, edge-double-click and outward-drag restore
  rules from U40 stay. Tests: neither header draws a count; a left panel's chevron sits at its right
  edge and a right panel's at its left; folding and restoring keeps the expanded width.
  Session notes: `panel_header` and `count_chip` in `apps/furnicraft/src/main.rs` draw the current
  row; `node_count` is cached only for that chip. `PanelSide` already names which edge a panel sits
  on, so the chevron side follows from it.
  now: committing the finished header and running the gate
  `files: apps/furnicraft/src/main.rs, crates/ui/src/theme.rs` — `873515c`

- `[x]` **U49** the interface speaks English and Russian: one typed message table behind a language
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
  `files: crates/ui/src, apps/furnicraft/src, xtask/src/main.rs` — `c027309`

- `[x]` **U46** the icon set catches up with the new surfaces: the header, the ribbon groups, the
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
  `files: assets/icons, crates/ui/src/icons.rs` — `21a2d6a`

- `[x]` **U48** the tree gains the eye and the padlock: every row of the scene tree carries a
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
  `files: crates/ui/src/scene_tree.rs, apps/furnicraft/src/scene_build.rs` — `6b5c502`

- `[x]` **X32** the cross on a tab does not close it, the File menu offers no Close and no Exit, the active tab is barely distinguishable and the new-scene button is a text label
  Done: the cross closes its tab (a click on the cross is a close, a click on the body
   activates), File carries Close tab and Exit (Exit asks once when any scene has unsaved
   changes), the active tab draws an accent bar and an accent-tinted fill, and the new-scene
   button draws assets/icons/actions/add.svg instead of its label; a test proves the cross wins
   over the body and the two File entries land in the right path
  `files: apps/furnicraft/src/header.rs, apps/furnicraft/src/file_menu.rs, apps/furnicraft/src/main.rs, assets/icons/actions/add.svg` — `71a636f`

- `[x]` **U45** several scenes open at once: the tab row in the header holds one tab per open file,
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
  close and per-tab textures; (4) ADR 0081, the map row, the plan close and the gate.
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src/file_menu.rs` — `c6d3836`

- `[x]` **S22** `main.rs` keeps the frame loop and gives up its chrome — rules: B1, B4, B8, H5.
  Why: after S21 the file is ~1980 lines, and four chrome pieces each have their own reason to change:
  the status bar (`StatusInfo`, `StatusOut`, `status_state_ui`, `apply_status`, `hint`, `rule`), the
  folding side panels (`PanelHead`, `side_panel`, `panel_header`, `count_chip`, `collapsed_strip`), the
  ribbon-band geometry (`strip_rect`, `strip_area`, `uncovered`, `fit_uncovered`, `view_region`) and
  the shortcut readers (`Shortcuts`, `take_shortcuts`, `take_drawing_shortcut`) — F1, B4.
  Shape: they move to `apps/furnicraft/src/status_bar.rs`, `side_panels.rs`, `strip.rs` and
  `shortcuts.rs`; `main.rs` 1980 -> ~1460, and U45 then takes `viewport` (635) onto `Document` and
  leaves the shell near 800.
  Done: suite unchanged (the same 34 tests, 14 of them left in `main.rs`); each module states
   what it draws or reads and carries `//! Changes when:`; the `plan/map.md` rows for the
   status bar, the folding panels, the split views, the ribbon and the scene tree name the new
   files. Measured: `main.rs` 2608 -> 2037 production and 859 -> 265 test lines;
   `status_bar.rs` 287, `side_panels.rs` 159, `strip.rs` 114, `shortcuts.rs` 82 production
   lines, no ceiling of `plan/refactor.md` §3 crossed.
  Commits: one move.
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src` — `a543599`

- `[x]` **S21** the shell's Home surface moves out of `main.rs` — rules: B4, B8, H2, H5.
  Why: `home_surface` is one 632-line function inside `impl FurniCraftApp` (1620 lines, 40 fields),
  four times B4's 150-line function ceiling (F1).
  Shape: `home_surface` becomes a free function in `apps/furnicraft/src/home.rs` (~640) over the state
  it reads, called from `ui()`; `main.rs` 2609 -> ~1980.
  Done: the existing Home tests move unchanged; `home.rs` states that it draws the start surface and
  returns one action, that it never touches the document, and `//! Changes when:`.
  Commits: one move.
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src` — `c83ace5`

- `[x]` **S20** `ui/contour_edit.rs` gives the drawing draft its own module — rules: B1, B4, B8.
  Why: 1112 production lines in one file cover two operations: editing an existing contour
   (`ContourEdit`, `ContourTarget`, `ContourDraft`, `ContourEntry`) and drawing a new one
   (`DrawTool`, `DrawDraft`, 425-1112) — a new draw tool and a new grip rule reopen one file,
   which is the crossing the audit measured beyond the filed findings.
  Shape: `DrawTool` and `DrawDraft` with their impls move to `crates/ui/src/draw.rs` (~690);
  `contour_edit.rs` 1112 -> ~425; `lib.rs` re-exports `DrawTool` and `DrawDraft` so the shell changes
  at most one `use`.
  Done: the draft tests move unchanged; both headers cite the decisions they implement (ADR 0048,
  0052, 0076, 0077) and state `//! Changes when:`.
  Commits: one move.
  `files: crates/ui/src/contour_edit.rs, crates/ui/src/lib.rs` — `ecf924f`

- `[x]` **S19** `crates/ui/src/lib.rs` becomes a re-export surface and `Editor` splits by mode — rules:
  B1, B3, B4, B5, B8.
  Why: 1085 production lines with `impl Editor` at 966, 18 fields and 60 public methods: four
   pointer modes, ribbon state and eight cards in one impl (F2, B4, B5).
  Shape: the `Editor` type and its state move to `crates/ui/src/editor/mod.rs`; the methods
   split by what they own — `selection.rs` (select, enter and exit a level, `retain_open`),
   `mesh.rs` (the mesh edit mode), `contour.rs` (the contour edit mode and its grips),
   `drawing.rs` (the draft and its commit), `creation.rs` (armed container, primitives, panels,
   assemblies, cameras, lights) and `cards.rs` (`show_scene_tree`, `show_card`, `run_action`);
   `lib.rs` 1085 -> ~150 and holds `mod` and `pub use` only (B3).
  Done: suite unchanged apart from paths and `use` lines (T1); the crate header still states the layer
  and that the UI dispatches and never writes the document; every new header names its mode and
  `//! Changes when:`; no module over 600 lines.
  Commits: one move; T2's sibling test files are optional and never in the same commit.
  `files: crates/ui/src/lib.rs, crates/ui/src` — `bc3b179`

- `[x]` **S18** `ui/lib.rs`'s `Editor` gives up the document rules it holds — rules: C7, D1, B1, H2.
  Why: three document rules live inside the UI's edit session (F2): `placement_in_parent` repeats the
  model's frame composition, `NEW_PANEL_LENGTH_MM`/`NEW_PANEL_WIDTH_MM` (600 x 400) are a document
  default held in the UI, and `resolve_viewport_pick`, `creation_target` and `child_containing` are the
  picking and creation-target rules (C7: the UI never holds a document default).
  Shape: `placement_in_parent` becomes `Project::placement_in_parent` beside `world_placement`
   in `crates/model/src/project/query.rs`; `default_panel_size` becomes
   `PanelCreate::default_nominal_size` in `crates/engine/src/panel_cmds/panel.rs` and the UI
   asks it; the pick and target helpers move to `crates/ui/src/target.rs`. Measured:
   `ui/lib.rs` 1158 -> 1085 production lines, `target.rs` 75, `impl Editor` still 966, so S19
   follows.
  Done: a model test for a rotated parent and for the root's identity frame; a created panel is still
  600 x 400; suite unchanged; the palette builds the same params.
  Commits: one move.
  `files: crates/ui/src/lib.rs, crates/model/src/project/query.rs, crates/engine/src/panel_cmds/panel.rs` — `cbc253e`

- `[x]` **S17** `render/lib.rs` gives up the vertex builders — rules: B1, B4, B8.
  Why: after S16 the file was 1136 production lines (`2c183d4`) and still held the CPU-side
   builders (`edge_geometry`, `shadow_geometry`, `world_vertices`, `flat_vertices`,
   `grid_vertices`, `polyline_vertices`, `mesh_centre`, `stencil_slot`, `edge_color`,
   `sort_back_to_front`).
  Shape: they moved to `crates/render/src/geometry.rs` (433 production lines); `render/lib.rs`
   1136 -> 742 and `impl ViewportRenderer` stays at 421, so both the file and the impl clear
   B4's ceilings (1000, 800).
  Done: suite unchanged; `geometry.rs` states that a builder reads frame data and writes vertices,
  never a device, and names the units it works in (H1, H2, H6).
  Commits: one move.
  `files: crates/render/src/lib.rs` — `1f361e1`

- `[x]` **S16** `render/lib.rs` gives up the pipelines and the GPU buffer helpers — rules: B1, B4, B8, H2.
  Why: 1446 production lines (F5) mix the renderer with eleven pipeline constructors, the depth-state
  builders and the buffer wrappers, so a shader change and an upload change reopen one file.
  Shape: `create_triangle_pipeline`, `create_line_pipeline`, `create_shadow_pipeline`, the
  `*_depth_state` builders, `face_target`, `vertex_layout`, `FACE_DEPTH_BIAS` and the stencil constants
  move to `crates/render/src/pipelines.rs` (~300); `GpuMesh`, `GpuLineList`, `GpuTriangles`,
  `line_list`, `triangle_list` and `clamp_size` move to `crates/render/src/gpu.rs` (~180);
  `render/lib.rs` 1446 -> ~1000.
  Done: suite unchanged; `pipelines.rs` states the depth, blend and stencil contract it encodes and
  `gpu.rs` states that it owns device objects and no frame data (H2, H6).
  Commits: one move.
  `files: crates/render/src/lib.rs` — `2c183d4`

- `[x]` **S15** `scene/lib.rs` gives up the camera and the ray — rules: B1, B4, B6, B8, H2.
  Why: 1277 production lines and 82 public items in one file cover the camera and its projection, the
  ray, picking, the grid, the lights, the contact shadows and the frame (F5).
  Shape: `Camera`, `Projection`, `FixedBasis`, `CameraError`, the fov, pitch and distance constants and
  the projection helpers move to `crates/scene/src/camera.rs` (~600); `Ray`, `ray_segment`,
  `ray_triangle`, `ray_hit_drawable` and `ray_hit_polyline` move to `crates/scene/src/ray.rs` (~180);
  `scene/lib.rs` 1277 -> ~520 (grid, lights, shadows, drawables, `SceneFrame`), under the ceiling, so
  no further split is filed.
  Done: suite unchanged; `scene/lib.rs` re-exports every moved name; both headers state millimetres,
  radians, near and far planes and the pick radius tolerance (H2).
  Commits: one move.
  `files: crates/scene/src/lib.rs` — `7372e80`

- `[x]` **P10** the gate follows the links the plan relies on
  Why: The gate checks that a hint names a path and that a map row names a file, and nothing
   else: five closed tasks point at files a later move deleted (`xtask/src/task.rs`,
   `geom/region.rs`, `panel_cmds.rs`), S14's own arithmetic disagrees with the tool's report
   (547 and 172 against 574 and 212), `plan/refactor.md` still offers `geom/region.rs` as B1's
   example, S20 cites an F10 the audit never filed, `plan/questions.md` is read by nothing, and
   `base` may be any ancestor, so a plan can describe a tree a hundred commits old and stay
   green.
  Shape: Every `files:` hint names a path that exists, closed entries included; a closed
   entry's ADR citation names a decision in `docs/decisions/`; `STATE:` carries only its six
   keys; `plan/questions.md` and `plan/debt.md` carry no status marker and no `STATE:` line,
   because they are not the plan; `state=idle` with `base` behind `HEAD` passes only when every
   commit in between touched `plan/` and nothing else.
  Done: The five dead hints point at the code that holds the work now; S14's numbers and B1's
   example are what the tree says; the plan stays green through `cargo xtask ci`; one test per
   rule.
  Commits: five: the repair path, the missing verbs, the hint splice, the links, the section
   rewrite.
  `files: state-contract/src/lib.rs, state-contract/src/validate.rs, state-contract/src/git.rs, plan/refactor.md, plan/BACKLOG.md, AGENTS.md` — `e353f02`

- `[x]` **S14** `engine/panel_cmds.rs` gives up its banding commands — rules: B1, B4, B6, B8.
  Why: 1143 production lines and 21 commands in one file with no test module, 547 of them banding
  (`panel.edge_banding.*`, `panel.corner_banding.*`, `panel.ring_banding.*`, `panel.corner.*`,
  `panel.bow.*`, lines 596-1142) — B4's ceiling and B6's public surface at once.
  Shape: the ten banding commands move to `crates/engine/src/panel_cmds/banding.rs`; the eleven
   that remain divide by value family into `panel.rs` (creation, outline, name, size, material)
   and `machining.rs` (the four records), because `mod.rs` holds declarations and re-exports
   only (B3) and 580 lines of logic is not one family (B1); `panel_cmds/mod.rs` re-exports
   every id and spec, so the `COMMANDS` table in `commands.rs` keeps its paths; `panel_cmds`
   1143 -> mod.rs 44 + panel.rs 411 + banding.rs 574 + machining.rs 212 production lines (1241:
   every new module carries its own header and imports), none past 600.
  Done: the registry test still finds every id; suite unchanged; `mod.rs` holds declarations and
  re-exports only (B3) and `banding.rs` states its one reason to change (H5).
  Commits: one move.
  `files: crates/engine/src/panel_cmds/mod.rs, crates/engine/src/panel_cmds/panel.rs, crates/engine/src/panel_cmds/banding.rs, crates/engine/src/panel_cmds/machining.rs` — `4e6d535`

- `[x]` **P9** the tool documents itself — rules: B1, C7.
  Why: the contract is written three times (`AGENTS.md` §1/§6.1, `templates/AGENTS.section.md`, the
  crate README), `plan/state.conf` does not exist although a second project needs its layout, and the
  usage blocks are hand-written.
  Shape: `cli::Config` with defaults (`runner`, `decisions`, `notes`) overridable from
  `plan/state.conf`; `agents` prints the embedded `templates/AGENTS.section.md` with the placeholders
  filled, `init` scaffolds a plan that passes the gate; `AGENTS.md` and the root `README.md` keep the
  invariants and point at the tool instead of repeating it.
  Done: a fresh `state-contract init` in an empty directory passes `check`; `agents` prints the
  section with the project's runner; `AGENTS.md` loses the duplicated command list; suite and
  `cargo xtask ci` green.
  Commits: one.
  `files: state-contract/src/main.rs, state-contract/README.md, AGENTS.md` — `c27002f`

- `[x]` **P8** the plan tool is one crate a project imports, not code in `xtask` — rules: B1, B3, B7, C7.
  Why: the write verbs, `list`, the agent form (`--md`/`--paths`), the code facts and the ADR index
  check live in `xtask` — 1 500 lines a second project would have to copy — while the crate that owns
  the format exposes only three read commands and two hand-written usage blocks that already differ.
  Shape: `state-contract/src/cli/` gains `table` (a `Command` list that drives dispatch, `--help` and
  `help <verb>`), `read` (status, dashboard, task, list, `--md`, `--paths`), `write` (the ten verbs),
  `facts` (production and test lines, the last commit, the ADR titles) and `docs`; the crate's own
  binary runs the whole table, and `xtask` keeps only its product rules (`layers`, the C-vendoring
  check, the `ci` step list) registered into the same table.
  Done: `cargo xtask <every command it has today>` prints the same output; `cargo xtask help` and
  `cargo xtask help task` are generated from the table; `cargo run -p state-contract -- help` lists
  the same commands minus the product ones; `xtask/src` holds no plan logic; the suite moves with the
  code and `cargo xtask ci` is green.
  Commits: one or two.
  `files: state-contract/src/main.rs, state-contract/src/lib.rs, xtask/src/main.rs` — `123afcb`

- `[x]` **P7** the gate checks the plan it already parses — rules: B1, C7.
  Why: `plan/map.md` is unchecked although a stale row sends a reader to a file that no longer exists;
  `## Next` in `plan/STATUS.md` is the first prose a session reads and can name another task than
  `STATE`; a debt tag with a typo silently drops out of the task report's selection; the ADR index can
  miss a new decision.
  Shape: `validate.rs` gains three checks — a map row naming a path that does not exist (only
  `.rs`/`.md`/`.toml` tokens and existing directories count), the first task id of `## Next` equal to
  `STATE: task=`, and every debt tag declared in the `Tags:` line; `xtask docs` compares
  `docs/decisions/README.md` with the files it indexes.
  Done: each check has a fixture that fails and the current plan still passes; `cargo xtask ci` green.
  Commits: one.
  `files: state-contract/src/validate.rs, state-contract/src/lib.rs, xtask/src/main.rs` — `5df8549`

- `[x]` **P6** the agent writes the plan through the tool, never by hand — rules: B1, B8, C7.
  Why: 194 commits touched `plan/BACKLOG.md` by hand, and a close is five edits — the marker, the
  hash, the move to `## Done`, `STATE` and deleting `SESSION.md` — each one a chance to break the
  order the gate enforces.
  Shape: `state-contract/src/edit.rs` splices line ranges in `plan/BACKLOG.md` and `plan/STATUS.md`,
  so a command rewrites only its own lines and the rest of the file stays byte-identical;
  `xtask task add|start|set|step|note|close|reopen|drop` and `xtask state set` build the new text,
  validate it with the gate in memory and write it through a temp file and a rename. Every verb takes
  `--dry-run` and prints its diff; `plan/.lock` keeps two writers apart.
  Done: each verb has a test that a refused command writes nothing; a round-trip test proves
  `parse(edit(parse(x)))` and that untouched bytes stay; `AGENTS.md` says the plan is written only
  through `cargo xtask`.
  Commits: one per verb family.
  `files: state-contract/src/parse.rs, state-contract/src/task.rs, xtask/src/main.rs` — `a35f3d5`

- `[x]` **P5** a session start needs no reasoning the tool can do — rules: B1, B7, C7.
  Why: `status` drops `resume:` and prints no interrupted flag, so `AGENTS.md` §1.1 case 1 makes the
  reader compare `base`/`HEAD` and inspect `steps` by hand; `task` prints only its own note, so
  research on another task that touched the same files is invisible; there is no way to enumerate
  tasks, the report does not say how many debt or question entries it filtered out, and a done task
  does not say which commit closed it. `next:` is also computed for a task outside `## Now`, where it
  means nothing.
  Shape: `Summary` carries `interrupted`, computed once in `check_with` (the dashboard stops deriving
  it); `render` prints `goal`, `resume` and the flag. `TaskView` gains `related` — other tasks' notes
  whose text meets the same `touches` needles — and `closed` (hash, date and subject of a done task).
  `xtask task list` filters by section, status, phase and text; the human and `--md` forms print how
  many entries were filtered out; `next` is `None` outside `## Now`; `state-contract` denies the
  panic lints at the crate root.
  Done: a session start prints `interrupted: yes` and `resume:` while the note has open steps; another
  task's note that names one of this task's files appears under `RELATED`; `list --phase X
  --status open` prints one line per task; the report says `+N other entries (--all)`; a done task
  shows `closed:`; the suite covers each; `cargo xtask ci` green.
  Commits: one or two. `[x]` `c884b81`
  `files: state-contract/src/lib.rs, state-contract/src/task.rs`

- `[x]` **P4** `cargo xtask task` prints one task in full — rules: B1, B7, C7, C10.
  Why: starting S14 costs `status`, then a grep of `plan/BACKLOG.md`, `plan/map.md`, `plan/debt.md`,
  `docs/decisions/` and `git log`, plus the production-line loop in `plan/refactor.md`; the gate
  already parses every one of those files, so the reads are one command's work, not the agent's.
  Shape: `state-contract` parses the labelled fields of an entry (`Rules`, `Why`, `Shape`, `Done`,
  `Commits`), reads a per-task note `plan/notes/<ID>.md`, and renders one task as a block a human
  reads (`task`, and `dashboard --task <ID>`); `xtask` adds what the plan does not carry — production
  and test lines per hinted file, the last commit that touched it, the ADR titles — and an agent
  form (`--md`, `--paths`) that needs no grep. The gate checks a note's id, staleness and budget.
  Done: `cargo xtask task S14` prints the entry with its fields, both files' line counts and last
  commits, the map row, matching debt, the ADRs, the note and the next task; `--md` carries no box
  drawing and `--paths` one path per line; a note for an unknown or finished task fails `plan`;
  `status` prints the lead sentence instead of the whole entry; the suite covers the new parse.
  Commits: one. `[x]` `7df9401`
  `files: state-contract/src/lib.rs, state-contract/src/parse.rs, xtask/src/main.rs`

- `[x]` **S13** `model/contour_edit.rs` splits into corner moves and shape frames — rules: B1, B4, B8.
  Why: after S12 the module is ~1280 lines, over B4's ceiling, and holds two reasons to change: moving
  a corner of any ring (`CornerMove`, `move_corner*`, trim and split, `PathCorner`) and the constrained
  drags of a tagged figure (`CircleFrame`, `RectangleFrame`, `PolygonFrame`, `ArcFrame` and the numeric
  setters, ADR 0073, ADR 0075).
  Shape: the four frames and their drags and setters move to `crates/model/src/contour_frames.rs`;
  `contour_edit.rs` keeps `CornerMove`, the error, `impl Path`'s corner moves and `impl Contour`.
  Done: suite unchanged; both headers name the constraint they enforce (free or along the picked
  segment; a tagged figure keeps its own) and their `//! Changes when:`; the `plan/map.md` row for
  contour edit math names both files.
  Commits: one move.
  Measured: 1481 -> 811 (`contour_edit.rs`) + 707 (`contour_frames.rs`) production lines, both under
  the ceiling though neither reaches the 600 target, because the shared corner machinery
  (`with_corner`, `geometry_arc`, `distance`, `SUPPORT_TOLERANCE`, `Path::position`/`replaced`) has one
  definition in `contour_edit.rs` and is `pub(crate)` instead of copied. The tests stayed in
  `contour_edit.rs` (T2 forbids mixing a test move into a logic commit); they reach the moved code
  through `Path` and the frame values, so only the two private `RectangleFrame` axes widened to
  `pub(crate)`.
  `d8864be`
  `files: crates/model/src/contour_edit.rs, crates/model/src/contour_frames.rs`

- `[x]` **S12** `model/contour_edit.rs` stops computing the trigonometry `geom` already owns — rules:
  D1, D2, B8, H2, H4.
  Why: 1572 production lines with 27 `millimetres_f64`/`radians_f64` conversions and its own
  `arc_with_radius`, `arc_between`, `arc_midpoint`, projection (`clamp_to_curve`) and split
  (`split_document`), although `geom` has `Arc::with_start_point`, `with_end_point`, `split_at`,
  `Line::split_at`, `nearest_on_support` and `from_endpoints_sagitta` (F3); `model/contour.rs` still
  calls itself the only place a segment becomes millimetres.
  Shape: the arc-from-radius, arc-between, projection and split maths become free functions beside the
  segments in `crates/geom/src/segment.rs`, each taking its tolerance as an argument (D2);
  `contour_edit.rs` keeps the `Length`/`Degrees` conversion at the call and computes nothing
  (conversions 27 -> ~8), 1572 -> ~1280. The `model/contour.rs` header states that the document
  converts once at its edge and that the maths lives in `geom`.
  Done: every contour-edit test passes unchanged; if a test disagrees with the `geom` formula, stop and
  file an `X` task instead of bending the test (§12); `cargo test -p furnicraft-model -p furnicraft-geom`.
  Commits: one move.
  Measured: `geom/segment.rs` grew 701 -> 875 production lines and holds `Arc::with_radius_through`,
  `Arc::through_ends`, `Arc::sweep_to`, `Arc::midpoint`, `Segment::clamp_to_extent`,
  `Segment::support_point_within` and `point_on_circle`; `contour_edit.rs` drops 1571 -> 1481 and keeps
  no cos, sin or split trigonometry. The conversion target was optimistic: 25 -> 21
  `millimetres_f64`/`radians_f64`, and the 21 that remain are all the document edge §4.3 requires (the
  rectangle half-extents, a length ratio, the radius argument of an arc rebuild, `geometry_arc`). The
  339 model and 97 geom tests pass unchanged; the move exposed X31 and did not fix it.
  `[x]` `392ebff`
  `files: crates/model/src/contour_edit.rs, crates/geom/src/segment.rs`

- `[x]` **S11** `geom/region.rs` becomes the general boolean alone — rules: B1, B3, B4, B8, H4.
  Why: after S10 the file is ~1120 lines and still mixes the ring model `Region` with the boolean
  algorithm, so a change to "what a region is" and a change to "how sets combine" reopen one file.
  Shape: `Region`, `RegionSamples`, `RingKind`, `RegionError`, the area, containment and validation
  helpers and `loop_to_contour` move to `crates/geom/src/ring.rs` (~450); what remains is the one
  named algorithm, renamed `crates/geom/src/boolean.rs` (~700, inside B4's 600-1200 band); `geom/lib.rs`
  keeps every public path.
  Done: suite unchanged; `boolean.rs` cites the method it implements and the sweep it does not do
  (self-intersection, tolerance), with `//! Changes when:` (H4, H6); the `plan/map.md` row for planar
  regions points at `ring.rs` and `boolean.rs`.
  Commits: one move; the rename is part of the move, not a separate edit.
  `7f1bc27`
  `files: crates/geom/src/boolean.rs, crates/geom/src/ring.rs, crates/geom/src/lib.rs`

- `[x]` **S10** `geom/region.rs` gives up the exact rectangle booleans — rules: B1, B4, B8, H2.
  Why: 1660 production lines, the largest geometry module, holds two unrelated layers (F5): the exact
  orthogonal boolean (`Rect`, `RectError`, `total_area`, `union_rects`, `subtract_rects`,
  `connected_count`, `regions_from_rects` and its loop tracer, 432-980) and the general contour
  boolean over `Region` (981-1660).
  Shape: the rectangle layer moves to `crates/geom/src/rect.rs` (~570) with its tests; `region.rs`
  1660 -> ~1120; `geom/lib.rs` re-exports `Rect` and the rectangle functions.
  Done: the 23 rectangle tests move unchanged (T1); `rect.rs` holds 575 production lines, `region.rs`
  drops 1660 -> 1126, the moved block and the moved tests are byte-identical, and the 62 geom tests
  pass unchanged; `rect.rs` states that a corner is a point and never a sample, that its arithmetic
  is exact, and its `//! Changes when:` (H2, H4); the `plan/map.md` row and ADR 0043 name both files.
  `186a5bc`
  `files: crates/geom/src/boolean.rs, crates/geom/src/lib.rs`

- `[x]` **S9** `geom/contour.rs` keeps the ring and gives the segment family its own module — rules:
  B1, B4, B8, H2, H5.
  Why: 1046 production lines crossed B4's 1000-line ceiling and the file held two value families with
  74 public items between them: `Line`, `Arc` and `Segment` with their impls (108-775) and the ring
  `Contour` with its operations (776-1046).
  Shape: `Line`/`Arc`/`Segment`, their impls and the private `positive_angle` moved verbatim to
  `crates/geom/src/segment.rs` (1046 -> 701 production lines); `ContourError`, `Contour`, the joint
  tolerances and the sampling caps stayed in `contour.rs` (1046 -> 374); `EPSILON`,
  `SUPPORT_TOLERANCE` and `MAX_ARC_EXTENSION` moved with the code that reads them; `geom/lib.rs`
  re-exports both modules and now also `MAX_ARC_EXTENSION`, which a model doc link already named.
  Done: 97 geom tests pass with one `use` line added for the constants the test module used to inherit
  (T1, B8); both headers state their family, millimetres and radians, the tolerance that matters and
  `//! Changes when:` (H1, H2, H5); the `plan/map.md` row names both files; `cargo xtask ci` green.
  `4a16ee8`
  `files: crates/geom/src/contour.rs, crates/geom/src/segment.rs, crates/geom/src/lib.rs`

- `[x]` **S8** the shared test fixtures move into `furnicraft-testkit` — rules: C7, T1, C4.
  Why: `fn millimetres` was written 20 times across 8 crates, `open_path` 5 times and `sample_project`
  twice, although `furnicraft-testkit` exists for exactly this (F7). Test-only: no production line
  change, no ceiling crossed — the finding was duplicated knowledge in 26 test modules.
  Shape: the three helpers have one definition in `model::testing`, behind the non-default `testing`
  feature that `furnicraft-testkit` enables and re-exports them from; `model` itself cannot dev-depend
  on the testkit, because the cycle compiles the library twice and its types stop matching (ADR 0084).
  `crates/model/src/fixtures.rs` keeps the fixtures only the model's own tests need; `param` and `mesh`
  gained the testkit `[dev-dependencies]` entry, `model` reaches the helpers through the feature.
  A shape one crate needs (the 100 × 60 chain, the 600 × 400 one, the parameterised L) stays local and
  is built through `open_path`.
  Done: 20 `millimetres` copies and 4 `open_path` shapes collapse into 3 definitions; the suite is the
  same 1378 tests with no assertion touched (T1, B8); `cargo xtask layers` green.
  `22e0e71`
  `files: crates/model/src/testing.rs, crates/testkit/src/lib.rs`

- `[x]` **S7** the design rules become an ordered refactor plan instead of an analysis: a read-only
  audit re-measures the tree, checks every finding against the code as it stands then, and files one
  small behaviour-preserving subtask per finding (`S8`, `S9`, … in dependency order) in `## Now`, then
  stops. The audit commit touches `plan/` only — no source file and no test. It runs right after S6
  because it is cheap and gives the rest of `## Now` its order; a subtask that edits `main.rs`,
  `crates/ui/src/lib.rs` or `apps/furnicraft/src/file_menu.rs` lands before U45, which moves exactly
  those files into `Document` and `tabs.rs`, and U45 then re-plans against the smaller result; a
  subtask inside `geom`, `model`, `scene`, `render` or `param` is independent of U45.
  Findings measured on 2026-11-10, a baseline to re-measure and not a requirement: `F1`
  `apps/furnicraft/src/main.rs` 2609 production lines, `impl FurniCraftApp` ≈1620, `viewport` 637,
  40 fields, 113 functions, ≥8 reasons to change; `F2` `crates/ui/src/lib.rs` `Editor` 18 fields, 60
  public methods, `impl` 1039, four pointer modes plus ribbon state plus eight cards, and three
  document rules inside it (`placement_in_parent`, `NEW_PANEL_LENGTH_MM/WIDTH_MM`,
  `resolve_viewport_pick`); `F3` `crates/model/src/contour_edit.rs` 1572 production lines with 27
  `millimetres_f64`/`radians_f64` conversions and its own projection, split and arc-from-radius
  trigonometry, which L0 `geom` already owns, while the `model/contour.rs` header still claims to be
  the single conversion point; `F4` `Project` ≈90 public methods and `Panel` 63, so one new panel
  field edits `model/panel`, `model/project/panel_cmds`, `engine/panel_cmds` (id, label, params,
  enabled, run), the `COMMANDS` table, `engine/lib.rs`, `ui/panel_card` and often `ui/palette`; `F5`
  file-monoliths with no submodule — `geom/region.rs` 1660 (exact rectangle booleans and general
  contour booleans), `scene/lib.rs` 1277 (camera, ray, grid, light, shadow, frame, picking),
  `render/lib.rs` 1446; `F6` `ui/panel_card.rs` and `ui/object_card.rs` ≈989 each (`impl PanelCard`
  ≈857, `run_action` 74) and `ui/toolbars.rs` 982 (`show_group_row` 146); `F7` shared fixtures
  rewritten in four crates (`millimetres`, `sample_project`, `open_path`) although
  `furnicraft-testkit` exists. Clean at filing time and not a finding: `cargo xtask layers` green, no
  `kernel` or GPU edge in `model`, no `egui` or `model` edge in `render`, the UI dispatches and never
  writes the document, the kernel port is five methods, tests are 39% of the tree.
  Instructions: read `plan/refactor.md` in full before the audit — it holds the measurement recipe
  (§2), the boundary, data, clean-code, documentation and test rules (§3–§7) with ids (`B1`, `D1`,
  `C4`, `H2`, `T1`), the legibility test that defines done (§8), what to borrow from SolidCraft and
  what never to copy (§9), the audit procedure (§10), the subtask template (§11) and the order and
  stopping rules (§12). The goal there is one sentence: a tree a single maintainer can hold in their
  head, where a panel attribute is a three-file change. In short: one module one operation, one
  reason to change stated in the header, geometry below the data and never inside a document type
  (it fixes F3 structurally, the way SolidCraft's `Seg2Geom for Seg2` does), a move never shares a
  commit with a fix, the existing suite passes unchanged, and every new or moved module header names
  its contract, its tolerance, what it refuses and `//! Changes when: …`. Documented debt is not
  re-filed: `plan/debt.md` `[view-state]` already owns selection and open containers in `Editor`, and
  F4's `Project::apply(change)` rewrite touches every command, so it is a question for the owner
  (`plan/questions.md`), not a subtask.
  Re-verify at the start, because the tree will have moved: S6 rewrites the frame tests and the panel
  and inspector documents, so `model/project`, `model/panel` and ADRs 0009, 0026, 0052, 0058, 0082,
  0083 are re-read, not quoted; U45, if it lands first, moves `Session`, `Editor`, `viewport` and
  `FileMenu` into `document.rs`/`tabs.rs`, so F1 and F2 are re-measured on the post-U45 files and the
  audit files no split that U45 already performs; U46, U48, U49, U50 and U52 edit `toolbars.rs`,
  `scene_tree.rs`, `main.rs` and the cards, so a subtask over one of those re-measures it first; R1
  (`plan/rename.md`) renames 13 crates and moves `apps/` to `app/`, so every path here and in
  `plan/map.md` is re-read after it, never copied from this entry.
  Done when `cargo xtask ci` is green on the pre-audit tree, every finding carries a verdict and
  either a subtask id with its `files:` hint or a `plan/debt.md` line under `[design]`, the subtasks
  sit in `## Now` in dependency order, each cites the `plan/refactor.md` rule ids it obeys with the
  production-line number it must reach, `plan/map.md` still describes the tree, and `plan/STATUS.md`
  names the first subtask. Tests: `cargo xtask plan` accepts every new hint and id, and the audit
  commit touches only `plan/`.
  Audit 2026-11-10: re-measured `geom/region.rs` 1660, `model/contour_edit.rs` 1572,
  `render/lib.rs` 1446, `scene/lib.rs` 1277, `ui/lib.rs` 1159, `engine/panel_cmds.rs` 1143,
  `ui/contour_edit.rs` 1112 and `geom/contour.rs` 1046 production lines, `main.rs` 2609; F1–F7
  confirmed except F6 (the two cards at 989 and `toolbars.rs` at 982 are under B4's ceiling, their
  impls under 800) and F4 (one `apply(Change)` per area is an owner decision, filed in
  `plan/questions.md`). Verdicts: F1 -> S21 (Home) and S22 (the chrome), with the `viewport` function
  and the 40-field impl deferred to U45; F2 -> S18 (the three document rules) and S19 (the `Editor`
  split); F3 -> S12 (the maths down to `geom`) and S13 (the split); F5 -> S10, S11 (`region`), S15
  (`scene`), S16, S17 (`render`); F7 -> S8; the three crossings the audit measured beyond the filed
  set -> S9 (`geom/contour`), S14 (`engine/panel_cmds`), S20 (`ui/contour_edit`). `plan/debt.md`
  `[design]` carries the four no-action verdicts. `model/panel/mod.rs` (967) and
  `model/panel/contour.rs` (889) stay under the ceiling and were not filed. `plan/map.md` still
  describes the tree, so no row moved.
  `9deed44`
  `files: plan/refactor.md, plan/BACKLOG.md, crates/model/src/contour_edit.rs, crates/ui/src/lib.rs`

- `[x]` **S6** the frame contract is enforced and written down: the invariants the earlier steps proved
  one by one become one test module, and the documents stop describing the old behaviour. Tests: for
  every creation path (line, polyline, arc, rectangle, circle, polygon, contour, extrusion, primitive,
  panel, group, duplicate) the local zero is the creation anchor and `pivot_local` is zero;
  `vertex_world = translation + rotation * vertex_local` and
  `pivot_world = translation + rotation * pivot_local` for every payload; a geometry edit changes no
  component of `translation`, `rotation` or `pivot_local` (an object edit; a panel outline edit
  re-anchors by X30 and compensates the translation instead); a move changes only `translation`; a turn
  about a pivot keeps `pivot_world`; world-zero keeps vertices, pivot and rotation; duplicate copies
  local fields exactly; undo/redo restores the frame bit for bit; a round trip at the current schema
  keeps every invariant. Docs: ADR 0082 and 0083 (written
  with S5 and X26) are checked against the code, ADRs 0009, 0026, 0052 and 0058 are corrected,
  `PROJECT.md` §"Space and views", `AGENTS.md` §4.10 and the `plan/map.md` rows for the frame, the
  panel and the inspector are updated. Two commits: the invariants with any fix they force, then the
  documents and `cargo xtask ci`.
  `d7ad707`
  `files: crates/model/src/project/mod.rs, docs/decisions, PROJECT.md, plan/map.md, AGENTS.md`
- `[x]` **X30** a panel's frame follows its parameters, its pivot does not: `move_frame_to` shifted the
  pivot by `-min` when a contour edit moved the ring's minimum corner, so the pivot drifted off the
  base corner, and the inspector let every node author a pivot. Expectation: a panel's pivot is
  derived data. An outline edit, a resize, a contour replacement or a rectangle baked into a ring
  creates the bbox, anchors the outline at zero, compensates `translation` so the frame's corner
  keeps its world position and sets `pivot_local = 0`; machining shifts with the outline. A panel's
  pivot is not authored: `set_node_pivot`/`center_node_pivot` return `PanelPivotIsDerived`, both
  committed commands are disabled for a panel, `move_mesh_vertices` never reaches one, and a loaded
  panel with a non-zero pivot is a readable error (ADR 0083). Tests: an outline edit keeps every
  drawn world point and every machining point, `translation` is the new world base corner and the
  pivot is zero; a resize keeps the translation; the contour-create panel reloads after an outline
  edit; the pivot commands refuse a panel and change nothing; the load error.
  `524f58f`
  `files: crates/model/src/panel/mod.rs, crates/model/src/project/panel_cmds.rs, crates/model/src/project/node_cmds.rs, crates/model/src/project/mod.rs`
- `[x]` **X29** a group is anchored at its bounds centre: `group_nodes` builds the container with an
  identity placement while `Container::new` puts the pivot at the centre of the children's bounds, so
  the card shows `Translation (0, 0, 0)` while the pivot sits in the middle of the group — the same
  class of defect as a drawing's frame, and a nested group compounds it. Expectation: grouping
  re-anchors with `rebase_at_anchor(CenterOfBounds)` — the new container's translation is the
  parent-frame centre of the children's union, every child placement moves by `-centre`,
  `pivot_local = 0`, and the children's world transforms do not change. `Container::new` stops
  inventing a pivot (a new container is born at zero), so no caller gets a hidden centre; the
  pre-schema-6 migration keeps `default_container_pivot` and no later pass re-normalizes it. Tests:
  a group of two panels keeps their world bounds and reports the centre as its translation; a group
  inside a rotated container keeps world positions and the parent-frame centre; a group of objects and
  panels uses the same union; no child's rotation changes; one undo entry.
  `1e04142`
  `files: crates/model/src/project/node_cmds.rs, crates/model/src/container.rs, crates/model/src/tree.rs`

- `[x]` **X27** the inspector tells the local and the world truth: the Transform card's X/Y/Z is the
  placement *relative to the parent* and the Pivot card's X/Y/Z is the pivot in the node's own frame,
  so at a nested node nothing on screen says where the node is in the world or what point the gizmo
  turns about, and there is no way to zero a nested node's translation. Expectation: the editable
  fields are labelled `Translation (local to <parent>)`, a read-only `World position` row reads
  `project.world_placement`, the Pivot card adds a read-only `Pivot (world)` from `pivot_world`, and a
  new `node.placement.world_zero` command sets `translation` to zero in the parent frame, keeping the
  rotation (the owner split a rotation reset into its own future command); at the root that is the
  world origin, inside a container the read-only row shows the parent's origin. For a panel the pivot
  fields are read-only: `Local pivot = 0` with the note "base corner" and `Pivot (world) = World
  position`, because the pivot is derived from the geometry, not authored. The card never computes a
  world value itself. Tests: the world rows match `world_placement`/`pivot_world` after a gizmo move
  and a rotation about the pivot; world-zero on a root node and on a node inside a rotated parent
  leaves the world rotation composed exactly as before, leaves vertices and `pivot_local`
  bit-identical and is one undo entry; a panel's pivot fields cannot be edited; the command is in the
  registry and refuses the root.
  `62501ad`
  `files: crates/ui/src/placement_card.rs, crates/ui/src/pivot_card.rs, crates/engine/src/node_cmds.rs`

- `[x]` **X26** a panel is anchored at its base corner: a panel is a parametric entity, not a mesh —
  the core draws a closed contour, extrudes the thickness and then explicitly re-anchors the frame with
  `rebase_at_anchor(BBoxBaseCorner)`, so the local zero *is* the base corner (min-X/min-Y for a
  rectangle, the chosen bbox corner for any other ring), `pivot_local = 0`, and `translation` is the
  parent-frame position of that corner. This is the owner's exception to "a closed body pivots at its
  bounds centre", not to "the local zero is the pivot" (ADR 0083). `add_panel_contour` today subtracts
  the ring's minimum corner when it anchors the outline but never adds it back to the placement, so a
  panel made from a scene contour moves; it is rebased instead, and `panel.contour.create` places the
  panel over its source: the source's world placement converted into the target parent's frame, with
  `params.placement` composed on top as a parent-frame offset (identity means "exactly over the
  source"). Machining, banding and the nominal rectangle keep their panel-local frame. Tests: a new
  rectangle panel is at the parent origin with a zero pivot; a ring panel's world vertices equal the
  source contour's and its pivot sits at the bbox base corner; a ring whose bounds do not start at zero
  is anchored without moving a world point; a nested parent and a rotated panel placement both keep the
  world corner equal to `world_placement ∘ (0, 0, 0)`; one undo entry.
  `c992043`
  `files: crates/model/src/panel/mod.rs, crates/model/src/project/panel_cmds.rs, crates/engine/src/panel_cmds/panel.rs`

- `[x]` **X25** a primitive lands under the pointer in a nested container: `place_primitive` handed
  `WorkPlane::placement_at` — a world placement — to `object.primitive.create` as a placement relative
  to the parent, so an armed container that was moved or turned put the cube somewhere else.
  Expectation: the editor converts the placement with the parent's inverse exactly as X24 does, and
  `object.primitive.create` keeps its mesh centred with a zero pivot, so the primitive's translation is
  the parent-frame position of the clicked point. Tests: the same click at the root and inside a
  rotated, moved container puts the primitive on the pointer; its pivot is zero, its mesh bounds are
  symmetric about the local zero, and the document changes once.
  `5370454`
  `files: crates/ui/src/lib.rs, apps/furnicraft/src/gizmo_math.rs, crates/engine/src/object_cmds.rs`

- `[x]` **X24** a drawing is born at its own zero: a line drawn at (600, 400) stores
  `translation = (0, 0, 0)` and puts (600, 400) into `pivot_local`, so the card honestly reports a
  translation of zero and the node cannot be moved to the world origin by its translation; a drawing
  made inside a moved or rotated container lands somewhere else, because the editor passes the work
  plane's *world* placement where the command expects a placement *relative to the parent*.
  Expectation: `object.path.create` and `object.extrude.create` rebase the figure with
  `rebase_at_anchor(CreationPivot)` and store `Pivot::ZERO`, so an open line's local zero is its first
  point, a closed ring's is its bounds centre, and every world point stays on the clicked point; the
  editor converts the one world placement the shell hands it into the target parent's frame with
  `Placement::inverse(parent_world)` in `commit_drawing`. `object.path.extrude` turns a node into a
  body in place and leaves its frame alone. The command docs that call the path "local" and the
  `add_object_with_pivot` doc are corrected here; the ADRs that describe the old behaviour are
  rewritten in S6. Tests: a line at (600, 400) has its first point in `translation`, zero in
  `pivot_local` and vertices around zero; a closed rectangle has its bounds centre in `translation`; a
  figure drawn into a rotated, moved container has `world_placement ∘ vertices_local` on the clicked
  points and a parent-frame `translation`; extrusion keeps the frame of the path it grew from; each
  creation is one undo entry.
  `617a973`
  `files: crates/engine/src/object_cmds.rs, crates/model/src/project/object_cmds.rs, crates/ui/src/lib.rs`

- `[x]` **S5** one operation chooses a node's frame: a new `crates/model/src/frame.rs` holds
  `rebase_at_anchor(strategy)` and the three strategies the owner approved — `CreationPivot` (an open
  chain's first point, a closed ring's bounds centre), `BBoxBaseCorner` (the local min-X/min-Y corner,
  or a named corner on request) and `CenterOfBounds` (the centre of a group's children). One rule with
  a small adapter per content kind (a `Path`, a `Contour`, a container's children): move the content by
  `-anchor`, keep the rotation, set the translation to where the anchor was, and leave the pivot at
  zero. `Path::translated([Length; 2])` mirrors `Contour::translated` — both ends of a line move, an
  arc moves by its centre, ids and angles stay — and `Placement::inverse` inverts a rigid frame (the
  conjugate quaternion and `-R⁻¹t`), so `inverse(parent) ∘ world` is the parent-frame placement of a
  world frame; it is the missing half of the existing `Placement::compose`.
  No command calls the new code in this step, so nothing visible moves. Tests: an open chain anchored
  at its first point and a closed ring at its bounds centre keep every world point within a
  micrometre; a rebased contour keeps its segment ids, centres and angles; `inverse` round-trips
  through `compose` and maps a world point back to its local one under a rotated parent;
  `BBoxBaseCorner` answers min-X/min-Y by default and a named corner on request; `CenterOfBounds` of
  mixed panels and objects is the centre of their union. ADR 0082 records the strategies and the rule
  that a frame is chosen only at creation.
  `d6a09a5`
  `files: crates/model/src, crates/model/src/placement.rs, crates/model/src/contour.rs`

- `[x]` **X23** the ribbon's *Close* leaves the ring a draft: a polyline closed from the floating
  tool strip becomes a ring on screen but is never committed, so the closed chain stays a draft with
  the last rubber band still pointing at the strip and the document never learns about the figure
  (observed 2026-11-10; the status bar's *Close* and a click on the first point do commit).
  Expectation: closing is one operation — close, then commit — whichever entry point asked for it
  (ADR 0052: "*Close* closes and commits in one step"). Suspected cause: `Editor::close_drawing` is a
  public primitive that only closes; the pair is written out by hand in the status bar, inferred from
  `is_finished` for a click in the viewport, and forgotten by `ToolAction::CloseDrawing`, while the
  gizmo keeps its cached preview because a closed draft's `preview` answers `Ok(None)`. Fix: one
  shell method `close_draft` owns close+commit, the ribbon and the status bar call it, the gizmo
  reports a close instead of performing it, and `Editor::close_and_commit_drawing` reopens the ring
  when the commit refuses it. The floating strip also leaves a band of itself hoverable, so the rubber
  band can follow the pointer onto the toolbar: the strip blocks the pointer over its whole
  rectangle. Tests: the ribbon reports the close and writes nothing; `close_and_commit_drawing`
  closes and commits as one undo entry and a refused close changes nothing; a closed draft drops the
  cached preview; a pointer over the strip band does not hover the viewport behind it. ADR 0052
  already decides the behaviour, so this adds no record.
  `ed0e8af`
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src/draw_gizmo.rs, crates/ui/src/lib.rs, crates/ui/src/toolbars.rs`

- `[x]` **U51** the toolbars float over the design space: the ribbon is an `Order::Middle` overlay
  anchored to the window below the header instead of a top panel, so the scene image runs under it,
  a translucent ribbon blends with the furniture, and resizing a side panel never reflows or jumps
  the strip. The side panels are inset by the reserved band; the orientation widget and *Frame all*
  use the uncovered part of the view, the fit distance scaled by the covered share and the target
  panned half the covered height. The reserved height is deterministic from the open state
  (`toolbars::height`) and the measured rectangle takes over once egui has sized the area. Tests: the
  strip keeps its anchor and width across side-panel resizes and covers the rendered rect; a click on
  it never reaches the viewport; the drawn band equals the reserved one; frame-all keeps the bounds
  inside the uncovered view. ADR 0080.
  `f5270e2`
  `files: apps/furnicraft/src/main.rs, crates/ui/src/toolbars.rs, crates/ui/src/theme.rs`

- `[x]` **U63** the display style belongs to one view: `Leaf` carries `ShadingStyle`, the cluster's
  five style buttons read and write the active leaf's style, and `ViewCluster` stops owning a
  global style. `ViewportRenderer::render_views` takes a `ViewSpec { camera, region, shading }` per
  view and builds `StylePasses` inside the pass loop, so *Edges*, *Shaded*, *Wire*, *Hidden* and
  *Ghost* differ between neighbours while one texture and one edge buffer still serve the
  workspace. The diagnostic `all_edges` switch stays global. `SceneFrame::shading` remains the
  single-view fallback and follows the active leaf. Tests: two views keep different styles through
  a frame build and a style change reaches only the active leaf; the renderer's per-view pass
  mapping matches `StylePasses`; opening a scene keeps each layout's styles; the settings default
  applies to a new view and to the active one when changed.
  `009a73f`
  `files: crates/render/src/lib.rs, apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/view_cluster.rs, apps/furnicraft/src/main.rs`

- `[x]` **U62** the active view expands over the whole workspace and comes back: `Layout` gains
  `maximized: Option<LeafId>`; `regions()` returns one full-area rectangle for that leaf and
  `dividers()` is empty, so borders and picking see one surface. `toggle_maximized()` expands the
  active view and restores the tree unchanged — every ratio and camera stays where the user left
  it — and `set_kind` clears the mode. The cluster gains a pressed *Expand* / *Restore* button
  (`view/maximize`, `view/restore` drawn assets), `ClusterAction::ToggleMaximize` and the `Alt+W`
  shortcut; the settings shortcut table names it. A single-view layout disables the button and the
  key does nothing. Tests: expand returns one region on the whole area and no borders; restore
  returns the same four rectangles, ratios and cameras; a layout switch leaves the mode; `Alt+W`
  is consumed and the single layout refuses it; the icon files pass the existing footprint and
  coverage tests and the URI count is 74.
  `2cbc453`
  `files: apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/view_cluster.rs, apps/furnicraft/src/main.rs, crates/ui/src/settings.rs, assets/icons/view, crates/ui/src/icons.rs`

- `[x]` **U61** *Frame all* fits the objects the scene has now: the app stores `scene_bounds` from
  the latest `evaluate`, and `frame_all` computes `scene_build::home_camera(scene_bounds)` on every
  press, then frames only the active view. `Leaf::frame` keeps the view's projection kind: a
  perspective view stays perspective and an orthographic view stays orthographic with the fit
  height, while the direction and roll follow the existing rule. The one-shot `home_camera` field
  and `opening_camera` helper go away; `Layout::new`/`reset_cameras` take the fit camera and the
  opening view, so a standard `Single` view is still the opening preset. Tests: a larger scene
  gives a larger fit distance; a custom orthographic view stays orthographic and fits; a standard
  view keeps its direction and ortho; an empty scene still yields a usable camera. A standard view
  the user made perspective also keeps that projection.
  `33c2275`
  `files: apps/furnicraft/src/main.rs, apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/scene_build.rs`

- `[x]` **U60** every layout remembers its own views: `Layout` holds one `LayoutState` per
  `LayoutKind` (tree, leaves, active, next id) instead of one tree that `set_kind` reshapes. First
  use of a layout builds its standard state: `Quad` = top-left `Front`, bottom-left `Top`,
  top-right `Left`, bottom-right `Axonometric`; `TwoColumns` = left `Front`, right `Axonometric`;
  `TwoRows` = top `Front`, bottom `Axonometric`; `Single` = the settings opening view. Switching
  away and back restores the cameras, presets, active view and every split ratio the user left;
  opening a file refits every stored layout with `reset_cameras(home, opening_view)` while keeping
  each layout's standard presets. The active view is the only target: `main.rs` drops
  `pointer_leaf`/`target_leaf = hovered`; orbit, pan, zoom, `O`, `Home`, presets, selection, gizmos
  and the cluster act on `layout.active()`, a press of any button over a view activates it, a
  border press still resizes without changing it, and the wheel only zooms when the pointer is over
  the active view. Tests: the three split layouts carry their standard views and ortho; a layout
  round-trip restores camera, preset, ratio and active; opening a scene refits every layout; a
  gesture over an inactive view is ignored until a press activates it; a border drag keeps the
  active view. ADR 0079 records the layout state.
  `c014bf9`
  `files: apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/main.rs`

- `[x]` **U59** the standard views become the six the owner names: `OpeningView` gains `Left` and
  `Bottom` and takes the owner's eye convention — `Front` looks from `+Y` to `-Y`, `Left` from `+X`
  to `-X`, `Top` from `+Z` down, `Bottom` from `-Z` up, `Right` from `-X` to `+X` — while
  `Axonometric` stays the three-quarter default. `ALL`, `label()` and `direction()` carry all six;
  `is_standard()` names the five non-axonometric views. Applying a standard preset sets the camera
  orthographic without moving its target, distance or visible height (`Camera::set_orthographic`
  converts a perspective view by the same height rule as the toggle); the corner widget's axis balls
  do the same through `Leaf::look_from`, so a click on a world axis is an orthographic view. Standard
  views are defaults, not modes: `O` still toggles projection in every view and leaves the preset
  name alone. Tests: every direction and label; `ALL` has six; a standard preset and an axis snap
  are orthographic; an axonometric preset and a free direction keep the projection; the toggle works
  in a standard view and the preset stays; `set_orthographic` preserves the visible height.
  `7dc71d1`
  `files: crates/ui/src/display.rs, crates/scene/src/lib.rs, apps/furnicraft/src/view_layout.rs`

- `[x]` **U58** the corner widget gains the roll ring and its step setting: a functional ring on a
  52 pt radius in a 124 pt square rotates the camera about its view axis, always visible with the
  widget, with four fixed nodes at the top, left, bottom and right carrying 0°, 90°, 180° and 270°
  and an accent handle at the camera's roll. A node click sets the absolute roll (0° resets); a drag
  anywhere on the annulus turns the camera with the pointer, quantised to the step from the workspace
  settings. The drag accumulates the unwrapped visual angle and applies the difference from what was
  already applied, so a slow drag never loses a step and the ±180° seam does not jump; the axis balls
  keep their hit squares and win an overlapping hit, and a press on a node never starts a hand turn.
  `Gizmo` owns the drag state; `GizmoAction` gains `RollTo(Degrees)` and `RollBy(Degrees)`. The shell
  passes the step and keeps the ring's drag out of the armed navigation and middle-button orbit.
  `ViewportSettings.roll_step_degrees: u32` defaults to 1, clamps to `1..=15` and is drawn in the
  Viewport section as a `DragValue`; the settings JSON needs no version bump because the container
  already has `#[serde(default)]`. Tests: node positions and degrees; a 37° drag at step 1 gives 37°
  and at step 5 gives 35°; a sub-step drag does nothing; the remainder carries; the seam does not
  jump; a node click wins over a drag; the ring hit excludes the balls; the handle follows the roll;
  orbit keeps the roll and a preset resets it; a ring drag is reported to the shell so it cannot
  orbit the viewport; the setting clamps and applies live.
  `8c329ad`
  `files: apps/furnicraft/src/gizmo.rs, crates/ui/src/settings.rs, apps/furnicraft/src/main.rs`

- `[x]` **U57** the camera rolls about its own view axis: `Camera` has no roll, so `right()` is
  horizontal, `up()` follows yaw and pitch only, and the readout's roll is always zero. Add a
  `roll: f64` (radians, view state like yaw and pitch, never document data): rotate the analytic base
  `right`/`up` about `forward` by the roll, keeping `up = right × forward`; a positive roll turns the
  image counter-clockwise. `roll()`, `roll_to(f64)` and `roll_by(f64)` normalise into `0..TAU`,
  ignore non-finite input and refuse a pinned `FixedBasis`, like `orbit`. `look_from` and
  `look_from_oriented` reset the roll; `orbit`, `pan_pixels`, `zoom`, `toggle_projection` and
  `with_projection` keep it. Presets, Home, Frame all and axis balls reset through
  `look_from_oriented`, orbit keeps. The readout takes `signed_degrees(camera.roll())`; ADR 0078
  records why the field is on the camera rather than a post-multiplied matrix, and ADR 0057 notes
  that a pole's canonical up is the zero-roll case. `Leaf::roll_to`/`roll_by` land with U58 because
  they would be dead code until the ring can call them.
  Tests: +90° sends `right` into `up` and the image of a world point from right to up; the view
  matrix stays orthonormal; picking and panning follow the rolled basis; a pole rolls without moving
  forward, eye or target; snaps reset while orbit keeps; a pinned camera ignores the roll; the
  readout shows the field.
  `6cd7ee6`
  `files: crates/scene/src/lib.rs, apps/furnicraft/src/view_layout.rs, apps/furnicraft/src/gizmo.rs`

- `[x]` **U56** the projection toggle joins the corner widget as a flat icon: the corner orientation
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
  `879f456`
  `files: apps/furnicraft/src/gizmo.rs, crates/ui/src/icons.rs, apps/furnicraft/src/main.rs`

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
  `files: crates/engine/src/panel_cmds/panel.rs, crates/model/src/panel/contour.rs`

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
