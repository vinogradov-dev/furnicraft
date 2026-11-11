# 0081. One document per tab; the window keeps only what it owns

- Status: Accepted
- Date: 2026-11-11

## Context

Every piece of a scene lived in the application struct: `Session`, `Editor`, the truck kernel, the
evaluation cache, the evaluated `SceneFrame`, the `Layout` and its cameras, the `ViewportRenderer`
and its `TextureId`, `FileMenu`, the preserved sections and five gizmos. Opening a file replaced
them all through one `adopt`, which is why only one scene could be open: a second document had
nowhere to live. S7 measured the cost as one 40-field `impl` and one 635-line `viewport` with at
least eight reasons to change (`plan/refactor.md`, finding F1).

The window's own state was mixed into the same struct: the pointer, the armed navigation gesture,
the transform mode, the panel folds, the measured ribbon band and the viewport rectangle.

## Decision

- **`apps/furnicraft/src/document.rs` holds one open scene.** A `Document` owns the session (project,
  undo history, dirty marker), the editor, the kernel and the cache, the frame, the layout with its
  cameras, its own renderer target and its own gizmos. It builds itself from a session and it can
  read a file, write one and close.
- **`apps/furnicraft/src/tabs.rs` holds the open documents and the close policy.** `TabId` is stable
  and never reused; `activate`, `add` and the three close answers (`Closed`, `NeedsConfirmation`,
  `LastTab`) are pure policy, testable with a plain payload and no GPU.
- **The shell keeps the window.** `FurniCraftApp` owns the tabs, the settings and its store, the
  Settings and Home surfaces, the palette and one `Shell`: the folds, the floating cluster, the
  transform mode, the pointer, the ribbon band, the viewport rectangle and the pending close
  question. A document borrows `Shell` for the frame it draws; it never stores any of it.
- **The display switches are pushed, not pulled.** The grid, the edge list and the contact shadow are
  window state, so the shell hands them to the active document once per frame (`sync_scene`) and the
  document keeps the last copy, so a frame rebuilt by a command, a card or a shortcut draws what the
  viewport shows.
- **Every tab has its own render target.** A document registers its `TextureId` from its own
  `TextureView` when it is built and the shell calls `Renderer::free_texture` when the tab closes;
  the wgpu handles themselves are one shared `RenderState`, which is a set of reference-counted
  handles, not a device per tab.
- ***New* and *Open* make a tab and never discard one.** Opening a path that is already open activates
  that tab instead of making a second copy of the same file, so two tabs cannot race to save it.
  `FileMenu` therefore has no confirmation left for New or Open; the one question it asks is a close
  with unsaved changes (`ConfirmClose`), owned by the shell because it is about a tab.
- **Closing the last tab empties it.** The workspace always has a document to draw, so the folds, the
  cluster and the pointer keep their home and no caller has to handle "no scene".
- **The split layout travels with the tab.** A tab keeps its own tree, ratios, cameras and styles, so
  switching tabs restores the workspace of that scene, and a scene opened in a new tab starts as one
  view. This supersedes the refit-on-open clause of ADR 0079: there is no longer a "new document in
  the old split", because the split belongs to the document, not to the window.

## Consequences

- Undo, redo, the selection, the camera, the dirty marker and the file path are per scene, and the
  header, the ribbon, the tree, the inspector and the status bar all read the active tab.
- The shell dropped from 2037 production lines to about 450 and `impl FurniCraftApp` from 113
  functions to 22; `viewport` (655 lines) and everything it drives now sit on `Document`, where S7
  can split it without the window in the way.
- A path dialog belongs to its tab: it is drawn while that tab is active and waits otherwise, which
  is the price of a document owning its own file menu. If that turns out to confuse, the dialog moves
  to the window like the close question did.
- Memory: one render target and one evaluation cache per open scene, freed when the tab closes. A
  workspace of many large scenes therefore costs what the user opened, which is the honest price.
- Each tab keeps its own `TransformMode` preview source but the mode itself is the window's: the
  frame follows whichever gizmo actually holds a drag, so a mode switch mid-drag cannot freeze the
  preview that is on screen.
